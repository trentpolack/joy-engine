// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scene terrain: the renderer's adapters for `RenderScene` terrain instances.
//!
//! Each live terrain lineage keeps one adapter with its GPU tile pool; surface snapshots stay shared. A frame prepares
//! every adapter before any pass opens, in two steps: [`SceneTerrain::prepare`] finds which instances the camera sees and
//! where they cast (shadow planning needs their bounds), then [`SceneTerrain::prepare_draws`] selects tiles once in each
//! terrain's local space for the camera and keeps, per view, the tiles that reach the camera or each shadow view. Passes
//! then record the same indexed, continuously morphed surface into the shadow views, the depth pre-pass, and the lit
//! scene pass (or, with deferred lighting, the G-buffer pass, in the lit model's encoding), which also writes the
//! scene-effect attachments when effects need them. Adapters for instances that leave the scene are released.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use joy_core::math::{
    DMat4,
    Mat4,
};
use joy_terrain::{
    TerrainError,
    TerrainSurface,
    TerrainView,
};

use crate::GpuContext;
use crate::camera::CameraFrame;
use crate::culling::{
    bounds_outside_view,
    outside_frustum,
    world_bounds,
};
use crate::mesh::Bounds;
use crate::pipelines::{
    CasterBias,
    DEPTH_FORMAT,
    SHADOW_FORMAT,
    ScenePipelines,
    SceneTargets,
    gbuffer_depth_state,
    scene_common,
};
use crate::scene::{
    RenderScene,
    TerrainInstance,
    TerrainInstanceId,
};
use crate::terrain::TerrainDiagnostic;
use crate::terrain::terrain_geometry::{
    ModelUniforms,
    TerrainGeometry,
    TerrainShadowView,
    TerrainStats,
    TerrainViews,
    terrain_bind_group_layout,
    terrain_buffer_layouts,
};
use crate::terrain::{
    TERRAIN_CASTER_SOURCE,
    TERRAIN_DEPTH_SOURCE,
    TERRAIN_LIT_GROUP,
    TERRAIN_LIT_VERTEX_ENTRY,
    terrain_geometry_source,
    terrain_lit_source,
};

/// The terrain group in the caster and depth pipelines, after their one pass group.
const GROUP: u32 = 1;
/// The camera's view index in each adapter's per-view draws; shadow view `index` is `index + 1`.
const CAMERA_VIEW: usize = 0;

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
    // The lit pipeline with scene-effect attachments, and the G-buffer pipelines; compiled when first needed.
    lit_targets: Option<wgpu::RenderPipeline>,
    gbuffer: Option<wgpu::RenderPipeline>,
    gbuffer_targets: Option<wgpu::RenderPipeline>,
    lit_layout: wgpu::PipelineLayout,
    lit_source: String,
    scene_format: wgpu::TextureFormat,
}

/// One scene terrain instance's resident resources, transform history, and this frame's inputs.
struct TerrainScenePass {
    geometry: TerrainGeometry,
    current_model: Mat4,
    previous_model: Option<Mat4>,
    world_bounds: Bounds,
    visible: bool,
    casts_shadow: bool,
    /// The snapshot, local-space selection view, and shading mode `prepare` found for this frame.
    surface: TerrainSurface,
    local_view: Option<TerrainView>,
    diagnostic: TerrainDiagnostic,
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
        Self {
            passes: HashMap::new(),
            frame: Vec::new(),
            pipelines: None,
            bias,
        }
    }

    /// Find which terrain instances in `scene` the camera sees and which cast shadows, write their transforms, and keep
    /// what [`prepare_draws`](Self::prepare_draws) needs. Call before shadow planning, which reads the returned caster
    /// bounds.
    ///
    /// `camera` is the stable (unjittered) frame camera and `logical_height` the viewport height in CSS pixels, so LOD
    /// quality does not depend on display density. Releases adapters for instances no longer in the scene. Fails before
    /// recording a pass for invalid instance settings or transforms.
    pub(crate) fn prepare(
        &mut self,
        gpu: &GpuContext,
        scene_pipelines: &ScenePipelines,
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
        // Snapshots of one terrain share a lineage, so a streamed surface keeps its adapter and tile pool.
        self.passes.retain(|id, pass| {
            live.iter()
                .any(|(live_id, instance, _)| live_id == id && instance.surface.lineage() == pass.geometry.lineage())
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
            let bounds = instance.local_bounds();
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
            pass.prepare(gpu, model, &view, instance);
            pass.world_bounds = world;
            pass.visible = visible;
            pass.casts_shadow = instance.casts_shadow;
            self.frame.push(id);
        }
        Ok(prepared)
    }

    /// Select each prepared terrain's tiles, keep the ones that reach the camera (`camera`, the stable view-projection)
    /// and each of `shadow_views` (every shadow view of the frame, in plan order, where casters the camera cannot see
    /// coarsen to the view's texels), upload new or edited tiles, and write their records. Call after shadow planning
    /// and before any pass opens.
    ///
    /// Fails before recording a pass when a device cannot hold the drawn tiles.
    pub(crate) fn prepare_draws(
        &mut self,
        gpu: &GpuContext,
        camera: Mat4,
        shadow_views: &[TerrainShadowView],
    ) -> Result<(), TerrainError> {
        let Some(pipelines) = &self.pipelines else {
            return Ok(());
        };
        for id in &self.frame {
            let pass = self.passes.get_mut(id).expect("prepared passes stay resident through the frame");
            let local_view = pass.local_view.expect("prepare found the frame's view");
            let views = TerrainViews {
                model: pass.current_model,
                camera: pass.visible.then_some(camera),
                shadows: if pass.casts_shadow { shadow_views } else { &[] },
            };
            pass.geometry.prepare(gpu, &pipelines.layout, &pass.surface, &local_view, views, pass.diagnostic)?;
        }
        Ok(())
    }

    /// Compile the pipeline the frame draws terrain with before a pass that uses it opens: the G-buffer pipeline writing
    /// `targets` when `deferred`, else the lit pipeline writing `targets`.
    pub(crate) fn ensure_lit(&mut self, device: &wgpu::Device, targets: SceneTargets, deferred: bool) {
        let Some(pipelines) = &mut self.pipelines else {
            return;
        };
        let slot = match (deferred, targets) {
            (false, SceneTargets::Color) => return,
            (false, SceneTargets::ColorAndAttachments) => &mut pipelines.lit_targets,
            (true, SceneTargets::Color) => &mut pipelines.gbuffer,
            (true, SceneTargets::ColorAndAttachments) => &mut pipelines.gbuffer_targets,
        };
        if slot.is_none() {
            let pipeline = if deferred {
                gbuffer_pipeline(device, &pipelines.lit_source, &pipelines.lit_layout, pipelines.scene_format, targets)
            } else {
                lit_pipeline(device, &pipelines.lit_source, &pipelines.lit_layout, pipelines.scene_format, targets)
            };
            *slot = Some(pipeline);
        }
    }

    /// Whether any terrain is visible to the camera this frame.
    pub(crate) fn has_visible(&self) -> bool {
        self.frame_passes().any(|terrain| terrain.visible)
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
        self.draw_visible(pass, pipeline)
    }

    /// Draw camera-visible terrain into the G-buffer pass in the lit model's encoding; returns draw calls and
    /// triangles. Expects the same groups as [`draw_lit`](Self::draw_lit), the lit model's stencil reference, and
    /// [`ensure_lit`](Self::ensure_lit) for deferred `targets`.
    pub(crate) fn draw_gbuffer(&self, pass: &mut wgpu::RenderPass<'_>, targets: SceneTargets) -> (usize, usize) {
        let Some(pipelines) = &self.pipelines else {
            return (0, 0);
        };
        let pipeline = match targets {
            SceneTargets::Color => pipelines.gbuffer.as_ref(),
            SceneTargets::ColorAndAttachments => pipelines.gbuffer_targets.as_ref(),
        };
        self.draw_visible(pass, pipeline.expect("ensure_lit compiled it"))
    }

    /// Draw every camera-visible terrain with `pipeline`; returns draw calls and triangles.
    fn draw_visible(&self, pass: &mut wgpu::RenderPass<'_>, pipeline: &wgpu::RenderPipeline) -> (usize, usize) {
        let (mut draws, mut triangles) = (0, 0);
        for terrain in self.frame_passes().filter(|terrain| terrain.visible) {
            draws += terrain.geometry.draw(pass, pipeline, TERRAIN_LIT_GROUP, CAMERA_VIEW);
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
            .map(|terrain| terrain.geometry.draw(pass, &pipelines.depth, GROUP, CAMERA_VIEW))
            .sum()
    }

    /// Draw the shadow-casting terrain tiles that reach shadow view `index` (in the plan order
    /// [`prepare_draws`](Self::prepare_draws) received, with matrix `view_projection`); returns draw calls.
    ///
    /// Expects the renderer's caster uniforms in group 0 at this view's dynamic offset.
    pub(crate) fn draw_shadow(&self, pass: &mut wgpu::RenderPass<'_>, index: usize, view_projection: Mat4) -> usize {
        let Some(pipelines) = &self.pipelines else {
            return 0;
        };
        self.frame_passes()
            .filter(|terrain| terrain.casts_shadow && !bounds_outside_view(terrain.world_bounds, view_projection))
            .map(|terrain| terrain.geometry.draw(pass, &pipelines.caster, GROUP, index + 1))
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
            geometry: TerrainGeometry::new(gpu, &instance.surface, layout)?,
            current_model: Mat4::IDENTITY,
            previous_model: None,
            world_bounds: crate::culling::EMPTY_BOUNDS,
            visible: false,
            casts_shadow: false,
            surface: instance.surface.clone(),
            local_view: None,
            diagnostic: TerrainDiagnostic::Material,
        })
    }

    /// Keep this frame's snapshot and the camera's view in the terrain's local space for selection, and upload the
    /// transform and its history.
    fn prepare(&mut self, gpu: &GpuContext, model: Mat4, view: &TerrainView, instance: &TerrainInstance) {
        let queue = gpu.queue();
        self.surface = instance.surface.clone();
        self.local_view = Some(local_view(model, view, instance));
        self.diagnostic = instance.diagnostic;
        self.previous_model = Some(if self.previous_model.is_some() { self.current_model } else { model });
        self.current_model = model;
        let flags = [if instance.receives_shadow { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0];
        let previous = self.previous_model.unwrap_or(model);
        self.geometry.write_model(
            queue,
            &ModelUniforms {
                model: model.to_cols_array_2d(),
                previous_model: previous.to_cols_array_2d(),
                flags,
            },
        );
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
        // The lit pass shares the scene pass's groups 0-2 and binds terrain as its material group; the others add the
        // terrain group after their own.
        let lit_layout = pipeline_layout(
            "terrain lit",
            &[Some(&scene.frame_layout), Some(&scene.shadow_layout), Some(&scene.image_lighting_layout), Some(&layout)],
        );
        let caster_layout = pipeline_layout("terrain caster", &[Some(&scene.caster_layout), Some(&layout)]);
        let depth_layout = pipeline_layout("terrain depth", &[Some(&scene.frame_layout), Some(&layout)]);
        let geometry = terrain_geometry_source(GROUP);
        let common = scene_common(device);

        let caster = terrain_pipeline(
            device,
            "terrain caster",
            &format!("{geometry}\n{TERRAIN_CASTER_SOURCE}"),
            &caster_layout,
            "vertexMain",
            None,
            &[],
            depth_state(
                SHADOW_FORMAT,
                wgpu::DepthBiasState {
                    constant: bias.constant,
                    slope_scale: bias.slope_scale,
                    clamp: 0.0,
                },
            ),
        );
        let depth = terrain_pipeline(
            device,
            "terrain depth",
            &format!("{common}\n{geometry}\n{TERRAIN_DEPTH_SOURCE}"),
            &depth_layout,
            "vertexMain",
            None,
            &[],
            depth_state(DEPTH_FORMAT, Default::default()),
        );
        let lit_source = terrain_lit_source(&common);
        let lit = lit_pipeline(device, &lit_source, &lit_layout, scene_format, SceneTargets::Color);
        Self {
            layout,
            lit,
            caster,
            depth,
            lit_targets: None,
            gbuffer: None,
            gbuffer_targets: None,
            lit_layout,
            lit_source,
            scene_format,
        }
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
        TERRAIN_LIT_VERTEX_ENTRY,
        Some(targets.fragment_entry()),
        &targets.states(scene_format),
        depth_state(DEPTH_FORMAT, Default::default()),
    )
}

/// Compile the terrain G-buffer pipeline for `targets`.
fn gbuffer_pipeline(
    device: &wgpu::Device,
    source: &str,
    layout: &wgpu::PipelineLayout,
    scene_format: wgpu::TextureFormat,
    targets: SceneTargets,
) -> wgpu::RenderPipeline {
    terrain_pipeline(
        device,
        "terrain g-buffer",
        source,
        layout,
        TERRAIN_LIT_VERTEX_ENTRY,
        Some(targets.gbuffer_entry()),
        &targets.gbuffer_states(scene_format),
        gbuffer_depth_state(),
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
#[allow(clippy::too_many_arguments)]
fn terrain_pipeline(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    layout: &wgpu::PipelineLayout,
    vertex_entry: &str,
    fragment_entry: Option<&str>,
    targets: &[Option<wgpu::ColorTargetState>],
    depth: wgpu::DepthStencilState,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let buffers = terrain_buffer_layouts();
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some(vertex_entry),
            compilation_options: Default::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: None,
            ..Default::default()
        },
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
            let bounds = instance.surface.bounds();
            let extent = (bounds.max.x - bounds.min.x).max(bounds.max.z - bounds.min.z);
            let center = (bounds.min + bounds.max) * 0.5;
            TerrainView {
                eye: center,
                target: center,
                target_radius: extent * 2.0,
                ..*view
            }
        }
    }
}

/// Gauss-Jordan inverse with partial pivoting; `None` when a pivot is nonfinite or below 1e-15 in magnitude.
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
