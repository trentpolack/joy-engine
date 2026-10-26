// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Paged terrain geometry: the GPU resources every scene terrain pass draws.
//!
//! All tiles reuse one shared indexed lattice. Their heights live in a pool of texture-array layers keyed by tile and
//! revision: a frame uploads only tiles it draws and has not seen at their current revision (or whose neighbors changed
//! under their layer's rings), reuses the layers of tiles drawn recently, and grows the array (re-uploading what it
//! draws) when a frame needs more layers than it has.
//!
//! Selection runs on the CPU once per frame. Each view (the camera, and every shadow view the terrain casts into) then
//! keeps only the tiles whose bounds reach it, grouped by LOD: one instanced draw per LOD per view, with each tile's
//! record (layer, placement, detail, and edges) as instance attributes. WebGPU, native backends, and WebGL 2 share one
//! path with no optional features.

use std::collections::HashMap;
use std::ops::Range;

use bytemuck::{
    Pod,
    Zeroable,
};
use joy_core::math::{
    Mat4,
    Vec3,
};
use joy_terrain::{
    TerrainError,
    TerrainGrid,
    TerrainNode,
    TerrainSurface,
    TerrainTile,
    TerrainView,
    TileKey,
    coarsen_terrain_nodes,
    create_terrain_grid,
    select_terrain_nodes,
    validate_terrain_view,
};
use web_time::Instant;
use wgpu::util::DeviceExt;

use crate::GpuContext;
use crate::culling::outside_frustum;
use crate::material::LIT_MODEL_ID;
use crate::mesh::Bounds;
use crate::terrain::terrain_tile_layers::{
    build_tile_layer,
    layer_height,
    layer_reads,
    layer_width,
};

/// Bytes of the material block (`MaterialParameters` in terrain-geometry.wgsl: three vectors).
const MATERIAL_BUFFER_BYTES: u64 = 48;
/// Bytes of one tile record (`TerrainPatch`: three vectors).
const PATCH_RECORD_BYTES: u64 = 48;
/// Tile records allocated before the first frame; the buffer doubles when a frame draws more.
const INITIAL_RECORDS: usize = 256;
/// Texture layers allocated before the first frame; the pool doubles up to the device limit.
const INITIAL_POOL_LAYERS: u32 = 32;
/// Bytes per height texel (`R32Float`).
const HEIGHT_TEXEL_BYTES: u64 = 4;
/// When the drawn tiles exceed the device's texture-array layers, the tolerance grows by this factor per retry, up to
/// this many retries.
const COARSENING_FACTOR: f64 = 1.5;
const MAXIMUM_COARSENING_STEPS: usize = 12;
/// A pooled revision no tile has, so the next frame that draws the tile uploads it again.
const STALE_REVISION: u64 = u64::MAX;
/// Shadow casters the camera cannot see draw at the coarsest level whose error stays within this many texels of the
/// cascade they cast into: finer geometry would not change the map, and no visible surface receives its own shadow from
/// them, so their error never shows as acne.
const SHADOW_CASTER_ERROR_TEXELS: f64 = 1.0;

/// Shading mode of the terrain passes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TerrainDiagnostic {
    /// Normal slope and altitude shading.
    #[default]
    Material,
    /// One color per fractional LOD (global detail level, so synthesized detail levels have their own colors).
    LodColors,
    /// LOD colors with the current cell grid overlaid.
    GridOverlay,
}

impl TerrainDiagnostic {
    /// The terrain shader's numeric diagnostic mode (0, 1, or 2).
    pub fn mode(self) -> f32 {
        match self {
            Self::Material => 0.0,
            Self::LodColors => 1.0,
            Self::GridOverlay => 2.0,
        }
    }
}

/// Copied per-frame statistics of one scene terrain. Memory totals count numeric payloads, not driver overhead.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TerrainStats {
    /// Tiles selected this frame, in every direction around the eye.
    pub patches: usize,
    /// Selected tiles the camera draws (the rest lie outside its frustum).
    pub visible_patches: usize,
    /// Distinct selected tiles any view draws: the tiles the GPU pool holds this frame.
    pub drawn_patches: usize,
    /// Tile draws across every shadow view this frame.
    pub shadow_patches: usize,
    /// Triangles drawn across every shadow view this frame.
    pub shadow_triangles: usize,
    /// Triangles the camera draws this frame (the selected index ranges of its tiles).
    pub triangles: usize,
    /// Triangles if every tile the camera draws drew its finest level.
    pub max_triangles: usize,
    /// Finest synthesized or authored level selected this frame (negative levels are synthesized detail).
    pub finest_level: i32,
    /// Screen-space tolerance the latest selection used, in CSS pixels; above the instance's when the drawn tiles had
    /// to be coarsened to fit the device's texture-array layers.
    pub tolerance: f64,
    /// Wall-clock selection, culling, and upload time of the latest prepare, in milliseconds.
    pub cpu_milliseconds: f64,
    /// Tiles resident in the surface snapshot.
    pub resident_tiles: usize,
    /// Resident CPU bytes: the snapshot's tile samples and errors, and the shared grid.
    pub resident_cpu_bytes: u64,
    /// Texture-array layers in the GPU tile pool.
    pub pool_layers: u32,
    /// Owned GPU buffer and texture bytes.
    pub resident_gpu_bytes: u64,
    /// Tiles uploaded by the latest prepare.
    pub uploaded_tiles: usize,
    /// Bytes uploaded by the latest prepare.
    pub frame_upload_bytes: u64,
}

/// The views one frame draws terrain into, as world-space view-projections in zero-to-one depth, and the terrain's
/// local-to-world transform.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TerrainViews<'a> {
    pub model: Mat4,
    /// The camera, when the terrain is visible to it.
    pub camera: Option<Mat4>,
    /// Every shadow view of the frame in plan order (directional cascades, then point-light faces), when the terrain
    /// casts shadows; empty otherwise.
    pub shadows: &'a [TerrainShadowView],
}

/// One shadow view terrain may draw into.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TerrainShadowView {
    /// World-space view-projection in zero-to-one depth.
    pub view_projection: Mat4,
    /// World size of one map texel; casters the camera cannot see coarsen to it. Zero keeps the camera's detail (point
    /// light faces, whose texels vary).
    pub texel: f32,
}

/// `MaterialParameters` in terrain-geometry.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct MaterialUniforms {
    coverage: [f32; 4],
    geometry: [f32; 4],
    materials: [f32; 4],
}

/// `TerrainPatch` in terrain-geometry.wgsl: one drawn tile's instance attributes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct PatchRecord {
    placement: [f32; 4],
    state: [f32; 4],
    edges: [f32; 4],
}

/// `TerrainModel` in terrain-geometry.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct ModelUniforms {
    pub model: [[f32; 4]; 4],
    /// The previous frame's transform, for motion vectors.
    pub previous_model: [[f32; 4]; 4],
    pub flags: [f32; 4],
}

/// The bind group layout of the terrain group: material block, height texture array, and model block.
pub(crate) fn terrain_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let uniform = |binding: u32, size: u64| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: wgpu::BufferSize::new(size),
        },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("terrain"),
        entries: &[
            uniform(0, MATERIAL_BUFFER_BYTES),
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float {
                        filterable: false,
                    },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            uniform(2, std::mem::size_of::<ModelUniforms>() as u64),
        ],
    })
}

/// The vertex layout of the shared grid: one `vec2f` lattice offset at location 0.
const GRID_ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];
/// The instance layout of tile records: placement, state, and edges at locations 1 through 3.
const PATCH_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![1 => Float32x4, 2 => Float32x4, 3 => Float32x4];

/// Vertex buffer layouts for every terrain pipeline: the shared grid, then one tile record per instance.
pub(crate) fn terrain_buffer_layouts() -> [Option<wgpu::VertexBufferLayout<'static>>; 2] {
    [
        Some(wgpu::VertexBufferLayout {
            array_stride: 8,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &GRID_ATTRIBUTES,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: PATCH_RECORD_BYTES,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &PATCH_ATTRIBUTES,
        }),
    ]
}

/// One instanced draw: tiles of one LOD, as a range of records.
#[derive(Clone, Debug)]
struct LodBatch {
    lod: u32,
    instances: Range<u32>,
}

/// One pooled tile: its texture layer, the revision uploaded there, and the frame that last drew it.
#[derive(Clone, Copy, Debug)]
struct PooledTile {
    layer: u32,
    revision: u64,
    last_frame: u64,
}

/// The height texture array and which tile each layer holds.
struct TilePool {
    // Lookup first, then the texture it describes.
    tiles: HashMap<TileKey, PooledTile>,
    free: Vec<u32>,
    capacity: u32,
    /// The newest tile revision of the snapshot the pool last refreshed against.
    newest_revision: u64,
    view: wgpu::TextureView,
    texture: wgpu::Texture,
}

/// Paged terrain resources and per-frame selection, drawn by any compatible terrain pipeline.
///
/// Dropping releases every owned GPU resource.
pub(crate) struct TerrainGeometry {
    // The terrain this adapter draws, the snapshot it last prepared, and this frame's selection.
    lineage: u64,
    surface: Option<TerrainSurface>,
    grid: TerrainGrid,
    nodes: Vec<TerrainNode>,
    /// Instanced draws per view: the camera first, then each shadow view in plan order.
    views: Vec<Vec<LodBatch>>,
    stats: TerrainStats,
    record_scratch: Vec<PatchRecord>,
    texel_scratch: Vec<f32>,
    frame: u64,

    // Bind group first: it references the buffers and the pool texture below.
    bind_group: wgpu::BindGroup,
    pool: TilePool,
    record_capacity: usize,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    records: wgpu::Buffer,
    material: wgpu::Buffer,
    model: wgpu::Buffer,
}

impl TerrainGeometry {
    /// Upload the shared grid and allocate the tile pool for `surface`'s layout.
    pub(crate) fn new(
        gpu: &GpuContext,
        surface: &TerrainSurface,
        layout: &wgpu::BindGroupLayout,
    ) -> Result<Self, TerrainError> {
        let device = gpu.device();
        let cells = surface.layout().tile_cells;
        let largest = layer_width(cells).max(layer_height(cells));
        if largest > device.limits().max_texture_dimension_2d {
            return Err(TerrainError::Unsupported(format!(
                "a {largest}-texel tile layer exceeds the {}-texel texture limit",
                device.limits().max_texture_dimension_2d
            )));
        }
        let grid = create_terrain_grid(cells);
        let initialized = |label: &str, contents: &[u8], usage: wgpu::BufferUsages| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage,
            })
        };

        // Static topology uploads once; the material, model, and record buffers are rewritten every frame.
        let vertices =
            initialized("terrain grid vertices", bytemuck::cast_slice(&grid.positions), wgpu::BufferUsages::VERTEX);
        let indices =
            initialized("terrain grid indices", bytemuck::cast_slice(&grid.indices), wgpu::BufferUsages::INDEX);
        let uniform = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let material = initialized("terrain material", &[0; MATERIAL_BUFFER_BYTES as usize], uniform);
        let identity = ModelUniforms {
            model: Mat4::IDENTITY.to_cols_array_2d(),
            previous_model: Mat4::IDENTITY.to_cols_array_2d(),
            flags: [1.0, 0.0, 0.0, 0.0],
        };
        let model = initialized("terrain model", bytemuck::bytes_of(&identity), uniform);
        let records = create_record_buffer(device, INITIAL_RECORDS);
        let capacity = INITIAL_POOL_LAYERS.min(device.limits().max_texture_array_layers);
        let pool = TilePool::new(device, cells, capacity);
        let bind_group = create_bind_group(device, layout, &material, &pool.view, &model);

        Ok(Self {
            lineage: surface.lineage(),
            surface: None,
            grid,
            nodes: Vec::new(),
            views: Vec::new(),
            stats: TerrainStats::default(),
            record_scratch: Vec::new(),
            texel_scratch: Vec::new(),
            frame: 0,
            bind_group,
            pool,
            record_capacity: INITIAL_RECORDS,
            vertices,
            indices,
            records,
            material,
            model,
        })
    }

    /// The terrain lineage this adapter draws.
    pub(crate) fn lineage(&self) -> u64 {
        self.lineage
    }

    /// Statistics from the latest prepare.
    pub(crate) fn stats(&self) -> &TerrainStats {
        &self.stats
    }

    /// Upload the world transform and shading flags read by scene passes.
    pub(crate) fn write_model(&self, queue: &wgpu::Queue, model: &ModelUniforms) {
        queue.write_buffer(&self.model, 0, bytemuck::bytes_of(model));
    }

    /// Select this frame's tiles for `view` (the terrain's local space, CSS pixels), keep each of `views`' tiles, upload
    /// new or edited tiles they draw, and write their records.
    ///
    /// Fails before any upload for invalid inputs or when the device cannot hold the drawn tiles.
    pub(crate) fn prepare(
        &mut self,
        gpu: &GpuContext,
        layout: &wgpu::BindGroupLayout,
        surface: &TerrainSurface,
        view: &TerrainView,
        views: TerrainViews<'_>,
        diagnostic: TerrainDiagnostic,
    ) -> Result<(), TerrainError> {
        validate_terrain_view(view)?;
        let start = Instant::now();
        self.frame += 1;
        let device = gpu.device();
        let queue = gpu.queue();

        // A device holds at most `max_texture_array_layers` drawn tiles (WebGL 2 guarantees 256); past that, relax the
        // tolerance until the drawn tiles fit rather than failing the frame.
        let limit = device.limits().max_texture_array_layers as usize;
        let mut selection_view = *view;
        let mut culled = cull_views(surface, select_terrain_nodes(surface, &selection_view)?.nodes, views);
        for _ in 0..MAXIMUM_COARSENING_STEPS {
            if culled.drawn.len() <= limit {
                break;
            }
            selection_view.tolerance *= COARSENING_FACTOR;
            culled = cull_views(surface, select_terrain_nodes(surface, &selection_view)?.nodes, views);
        }
        if culled.drawn.len() > limit {
            return Err(TerrainError::Unsupported(format!(
                "{} drawn tiles exceed the {limit}-layer texture array limit",
                culled.drawn.len()
            )));
        }
        let CulledViews {
            nodes,
            drawn,
            lists,
        } = culled;
        self.nodes = nodes;

        // Grow the pool before uploading when this frame draws more tiles than it holds; tiles whose rings read a tile
        // that changed since the last snapshot upload again.
        let cells = surface.layout().tile_cells;
        if drawn.len() > self.pool.capacity as usize {
            let capacity = (drawn.len() as u32).next_power_of_two().min(limit as u32);
            self.pool = TilePool::new(device, cells, capacity);
            self.bind_group = create_bind_group(device, layout, &self.material, &self.pool.view, &self.model);
        }
        if self.surface.as_ref().is_none_or(|previous| !previous.ptr_eq(surface)) {
            self.pool.refresh(surface);
        }
        self.surface = Some(surface.clone());

        // Upload tiles the pool lacks at their current revision; each drawn tile's layer is reserved for this frame.
        let mut uploaded = 0;
        let mut layers = vec![0_u32; self.nodes.len()];
        for &index in &drawn {
            let tile = surface.tile(self.nodes[index].key).expect("selection only draws resident tiles");
            let (layer, fresh) = self.pool.reserve(tile, self.frame);
            if fresh {
                build_tile_layer(surface, tile, &mut self.texel_scratch);
                self.pool.upload(queue, &self.texel_scratch, layer, cells);
                uploaded += 1;
            }
            layers[index] = layer;
        }

        // Records per view, grouped by LOD so each view draws one instance range per LOD. Shadow views draw the
        // casters the camera cannot see no finer than their texels resolve.
        let layout_geometry = surface.layout();
        let cells_log2 = layout_geometry.cells_log2();
        let mut seen = vec![false; self.nodes.len()];
        for index in &lists[0] {
            seen[*index] = true;
        }
        self.record_scratch.clear();
        self.views.clear();
        let mut shadow_triangles = 0;
        for (view, list) in lists.iter().enumerate() {
            let texel = if view == 0 { 0.0 } else { f64::from(views.shadows[view - 1].texel) };
            let coarsened = (texel > 0.0).then(|| {
                let budget = texel * SHADOW_CASTER_ERROR_TEXELS;
                coarsen_terrain_nodes(surface, &self.nodes, |index, node| {
                    if seen[index] { node.detail } else { coarsest_within(surface, node, budget) }
                })
            });
            let nodes = coarsened.as_deref().unwrap_or(&self.nodes);
            let mut batches = Vec::new();
            for lod in 0..=(cells_log2 as u32) {
                let first = self.record_scratch.len() as u32;
                for &index in list.iter().filter(|index| nodes[**index].lod() == lod) {
                    self.record_scratch.push(patch_record(surface, &nodes[index], layers[index]));
                    if view > 0 {
                        shadow_triangles += (self.grid.ranges[lod as usize].count / 3) as usize;
                    }
                }
                let end = self.record_scratch.len() as u32;
                if end > first {
                    batches.push(LodBatch {
                        lod,
                        instances: first..end,
                    });
                }
            }
            self.views.push(batches);
        }
        if self.record_scratch.len() > self.record_capacity {
            self.record_capacity = self.record_scratch.len().next_power_of_two();
            self.records = create_record_buffer(device, self.record_capacity);
        }
        if !self.record_scratch.is_empty() {
            queue.write_buffer(&self.records, 0, bytemuck::cast_slice(&self.record_scratch));
        }
        let materials = surface.materials();
        let uniforms = MaterialUniforms {
            coverage: [0.0, 0.0, LIT_MODEL_ID as f32, 0.0],
            geometry: [cells as f32, diagnostic.mode(), 0.0, 0.0],
            materials: [
                materials.rock_slope[0] as f32,
                materials.rock_slope[1] as f32,
                materials.snow_height[0] as f32,
                materials.snow_height[1] as f32,
            ],
        };
        queue.write_buffer(&self.material, 0, bytemuck::bytes_of(&uniforms));

        // Statistics count payloads, not driver overhead.
        let camera = &lists[0];
        let tile_triangles = |index: &usize| (self.grid.ranges[self.nodes[*index].lod() as usize].count / 3) as usize;
        let layer_bytes = u64::from(layer_width(cells) * layer_height(cells)) * HEIGHT_TEXEL_BYTES;
        let grid_bytes =
            std::mem::size_of_val(self.grid.positions.as_slice()) + std::mem::size_of_val(self.grid.indices.as_slice());
        let record_bytes = (self.record_scratch.len() as u64) * PATCH_RECORD_BYTES;
        self.stats = TerrainStats {
            patches: self.nodes.len(),
            visible_patches: camera.len(),
            drawn_patches: drawn.len(),
            shadow_patches: lists[1..].iter().map(Vec::len).sum(),
            shadow_triangles,
            triangles: camera.iter().map(tile_triangles).sum(),
            max_triangles: camera.len() * (cells as usize).pow(2) * 2,
            finest_level: self.nodes.iter().map(|node| node.key.level).min().unwrap_or(0),
            tolerance: selection_view.tolerance,
            cpu_milliseconds: 0.0,
            resident_tiles: surface.tile_count(),
            resident_cpu_bytes: (surface.tiles().map(|tile| tile.payload_bytes()).sum::<usize>() + grid_bytes) as u64,
            pool_layers: self.pool.capacity,
            resident_gpu_bytes: (u64::from(self.pool.capacity) * layer_bytes)
                + (self.record_capacity as u64 * PATCH_RECORD_BYTES)
                + grid_bytes as u64
                + MATERIAL_BUFFER_BYTES
                + std::mem::size_of::<ModelUniforms>() as u64,
            uploaded_tiles: uploaded,
            frame_upload_bytes: (uploaded as u64 * layer_bytes) + record_bytes + MATERIAL_BUFFER_BYTES,
        };
        self.stats.cpu_milliseconds = start.elapsed().as_secs_f64() * 1000.0;
        Ok(())
    }

    /// Draw one view's tiles with `pipeline` into a borrowed pass, binding the terrain group at `group`; `view` is 0 for
    /// the camera and `1 + index` for shadow view `index`. Returns the draw count.
    ///
    /// The caller binds any groups the pipeline needs below `group`. The pass is not ended or submitted.
    pub(crate) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        pipeline: &wgpu::RenderPipeline,
        group: u32,
        view: usize,
    ) -> usize {
        let Some(batches) = self.views.get(view).filter(|batches| !batches.is_empty()) else {
            return 0;
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(group, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_vertex_buffer(1, self.records.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        for batch in batches {
            let range = self.grid.ranges[batch.lod as usize];
            pass.draw_indexed(range.first..range.first + range.count, 0, batch.instances.clone());
        }
        batches.len()
    }
}

/// A selection split into views.
struct CulledViews {
    nodes: Vec<TerrainNode>,
    /// Indices of nodes some view draws, each once.
    drawn: Vec<usize>,
    /// Node indices per view: the camera first, then each shadow view.
    lists: Vec<Vec<usize>>,
}

/// Keep, for each view, the selected tiles whose bounds reach it.
fn cull_views(surface: &TerrainSurface, nodes: Vec<TerrainNode>, views: TerrainViews<'_>) -> CulledViews {
    let layout = surface.layout();
    let bounds: Vec<Bounds> = nodes
        .iter()
        .map(|node| {
            let tile = surface.tile(node.key).expect("selection only draws resident tiles");
            let (low, high) = layout.tile_footprint(node.key);
            Bounds {
                min: Vec3::new(low[0] as f32, tile.minimum() as f32, low[1] as f32),
                max: Vec3::new(high[0] as f32, tile.maximum() as f32, high[1] as f32),
            }
        })
        .collect();
    let reach = |view_projection: Mat4| -> Vec<usize> {
        (0..nodes.len()).filter(|index| !outside_frustum(bounds[*index], views.model, view_projection)).collect()
    };
    let mut lists = vec![views.camera.map(reach).unwrap_or_default()];
    lists.extend(views.shadows.iter().map(|view| reach(view.view_projection)));

    let mut used = vec![false; nodes.len()];
    for index in lists.iter().flatten() {
        used[*index] = true;
    }
    let drawn = (0..nodes.len()).filter(|index| used[*index]).collect();
    CulledViews {
        nodes,
        drawn,
        lists,
    }
}

/// The coarsest global level at which `node`'s tile stays within `budget` world units of the finest surface (its own
/// level when even that misses).
fn coarsest_within(surface: &TerrainSurface, node: &TerrainNode, budget: f64) -> f64 {
    let tile = surface.tile(node.key).expect("selection only draws resident tiles");
    let within = tile.errors().iter().take_while(|error| **error <= budget).count();
    f64::from(tile.first_error_level() + within.saturating_sub(1) as i32)
}

/// One drawn tile's record: its layer and placement, and its details relative to its own level.
fn patch_record(surface: &TerrainSurface, node: &TerrainNode, layer: u32) -> PatchRecord {
    let layout = surface.layout();
    let cells_log2 = f64::from(layout.cells_log2());
    let level = f64::from(node.key.level);
    let (low, _) = layout.tile_footprint(node.key);
    let step = joy_core::math::deterministic::exp2(level);
    PatchRecord {
        placement: [low[0] as f32, low[1] as f32, (layout.spacing[0] * step) as f32, (layout.spacing[1] * step) as f32],
        state: [layer as f32, (node.detail - level).min(cells_log2) as f32, level as f32, 0.0],
        edges: node.edges.map(|edge| (edge - level).min(cells_log2) as f32),
    }
}

impl TilePool {
    /// An empty pool of `capacity` layers for tiles of `cells` cells.
    fn new(device: &wgpu::Device, cells: u32, capacity: u32) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("terrain tile heights"),
            size: wgpu::Extent3d {
                width: layer_width(cells),
                height: layer_height(cells),
                depth_or_array_layers: capacity,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        Self {
            tiles: HashMap::new(),
            free: (0..capacity).rev().collect(),
            capacity,
            newest_revision: 0,
            view,
            texture,
        }
    }

    /// Mark pooled tiles whose layers read a tile that arrived or changed since the last snapshot (a newer revision),
    /// so they upload again the next time they draw.
    fn refresh(&mut self, surface: &TerrainSurface) {
        let changed: Vec<TileKey> =
            surface.tiles().filter(|tile| tile.revision() > self.newest_revision).map(|tile| tile.key()).collect();
        self.newest_revision = surface.tiles().map(|tile| tile.revision()).max().unwrap_or(0).max(self.newest_revision);
        if changed.is_empty() {
            return;
        }
        let layout = surface.layout();
        for (key, pooled) in &mut self.tiles {
            if changed.iter().any(|changed| layer_reads(layout, *key, *changed)) {
                pooled.revision = STALE_REVISION;
            }
        }
    }

    /// The layer for `tile` this frame, and whether it needs an upload. Reuses its layer, then a free layer, then the
    /// least recently drawn layer not drawn this frame. The caller guarantees capacity for every tile of the frame.
    fn reserve(&mut self, tile: &TerrainTile, frame: u64) -> (u32, bool) {
        if let Some(pooled) = self.tiles.get_mut(&tile.key()) {
            let stale = pooled.revision != tile.revision();
            pooled.revision = tile.revision();
            pooled.last_frame = frame;
            return (pooled.layer, stale);
        }
        let layer = match self.free.pop() {
            Some(layer) => layer,
            None => {
                let (&key, pooled) = self
                    .tiles
                    .iter()
                    .filter(|(_, pooled)| pooled.last_frame < frame)
                    .min_by_key(|(key, pooled)| (pooled.last_frame, key.level, key.z, key.x))
                    .expect("the pool holds at least as many layers as the frame draws tiles");
                let layer = pooled.layer;
                self.tiles.remove(&key);
                layer
            }
        };
        self.tiles.insert(
            tile.key(),
            PooledTile {
                layer,
                revision: tile.revision(),
                last_frame: frame,
            },
        );
        (layer, true)
    }

    /// Copy one tile's layer texels ([`build_tile_layer`]) into `layer`.
    fn upload(&self, queue: &wgpu::Queue, texels: &[f32], layer: u32, cells: u32) {
        let (width, height) = (layer_width(cells), layer_height(cells));
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: layer,
                },
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(texels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * HEIGHT_TEXEL_BYTES as u32),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }
}

/// An instance buffer of `records` tile records.
fn create_record_buffer(device: &wgpu::Device, records: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("terrain tile records"),
        size: records as u64 * PATCH_RECORD_BYTES,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// The terrain group binding the material block, the pool, and the model block.
fn create_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    material: &wgpu::Buffer,
    heights: &wgpu::TextureView,
    model: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("terrain"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: material.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(heights),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: model.as_entire_binding(),
            },
        ],
    })
}
