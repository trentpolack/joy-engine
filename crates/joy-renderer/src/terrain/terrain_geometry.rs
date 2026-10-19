// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Resident indexed terrain geometry: the GPU resources every scene terrain pass draws.
//!
//! All patches reuse one sample-space grid and the heights stay in a resident texture, so moving the camera changes
//! only patch records and index ranges, never vertex buffers. WebGPU-class backends select LODs with two ordered compute
//! dispatches that write patch records and indexed indirect draws; WebGL 2 runs the same policy on the CPU and uploads
//! the records. Either way, each draw binds its patch record at a dynamic uniform offset, so indirect draws keep
//! `firstInstance` at zero and need no optional features.

use bytemuck::{
    Pod,
    Zeroable,
};
use joy_core::math::Mat4;
use web_time::Instant;
use wgpu::util::DeviceExt;

use crate::GpuContext;
use crate::terrain::TERRAIN_SELECT_SOURCE;
use crate::terrain::terrain_asset::{
    TerrainAsset,
    TerrainError,
};
use crate::terrain::terrain_grid::{
    TerrainGrid,
    create_terrain_grid,
};
use crate::terrain::terrain_selection::{
    TerrainSelection,
    TerrainView,
    select_terrain,
    validate_terrain_view,
};

/// Bytes of the per-terrain scene block (`TerrainScene`: a matrix and four vectors).
const SCENE_BUFFER_BYTES: u64 = 128;
/// Bytes of one patch record (`TerrainPatch`: two vectors).
const PATCH_RECORD_BYTES: u64 = 32;
/// Stride between patch records; the largest uniform offset alignment any device may require.
const PATCH_SLOT_BYTES: u64 = 256;
/// Bytes of one indexed indirect draw command.
const INDIRECT_COMMAND_BYTES: u64 = 20;
/// Bytes of one compute metadata record (three vectors).
const METADATA_BYTES: u64 = 48;
/// Compute workgroup width in `terrain-select.wgsl`.
const COMPUTE_WORKGROUP_SIZE: u32 = 64;
/// Patch-record slots hold eight error levels (`tile_cells` up to 128).
const ERROR_SLOTS: usize = 8;

/// Shading mode of the terrain passes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TerrainDiagnostic {
    /// Normal slope and altitude shading.
    #[default]
    Material,
    /// One color per fractional LOD.
    LodColors,
    /// LOD colors with the current cell grid overlaid.
    GridOverlay,
}

impl TerrainDiagnostic {
    /// The shader's numeric mode (0, 1, or 2), matching the TypeScript `diagnostic` values.
    pub fn mode(self) -> f32 {
        match self {
            Self::Material => 0.0,
            Self::LodColors => 1.0,
            Self::GridOverlay => 2.0,
        }
    }
}

/// How [`TerrainStats::triangles`] was counted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TerrainTriangleCount {
    /// GPU selection is never read back, so the count is the all-finest upper bound.
    #[default]
    UpperBound,
    /// CPU selection counted the selected index ranges.
    Selected,
}

impl TerrainTriangleCount {
    /// Label used by overlays, matching the TypeScript text.
    pub fn label(self) -> &'static str {
        match self {
            Self::UpperBound => "upper bound (no readback)",
            Self::Selected => "selected",
        }
    }
}

/// Copied per-frame statistics of one scene terrain. Memory totals count numeric payloads, not driver overhead.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainStats {
    /// Resident leaf patches.
    pub patches: usize,
    /// Triangles drawn this frame (see `triangle_count_kind`).
    pub triangles: usize,
    /// Triangles when every patch draws its finest level.
    pub max_triangles: usize,
    /// Wall-clock selection and upload time of the latest prepare, in milliseconds.
    pub cpu_milliseconds: f64,
    /// Resident CPU bytes: samples and the shared grid.
    pub resident_cpu_bytes: u64,
    /// Owned GPU buffer and texture bytes.
    pub resident_gpu_bytes: u64,
    /// Bytes uploaded by the latest prepare.
    pub frame_upload_bytes: u64,
    /// Whether `triangles` is an upper bound or a selected count.
    pub triangle_count_kind: TerrainTriangleCount,
}

/// `TerrainScene` in terrain-geometry.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct SceneUniforms {
    matrix: [[f32; 4]; 4],
    eye: [f32; 4],
    geometry: [f32; 4],
    settings: [f32; 4],
    focus: [f32; 4],
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

/// GPU selection: compute pipelines, their inputs, and the indirect commands they write.
struct GpuSelection {
    evaluate: wgpu::ComputePipeline,
    reconcile: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
    indirect: wgpu::Buffer,
    // Kept alive for the bind group.
    _metadata: wgpu::Buffer,
    _details: wgpu::Buffer,
    _ranges: wgpu::Buffer,
}

/// The bind group layout for terrain group 3: scene block, height texture, dynamic patch record, and model block.
pub(crate) fn terrain_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let uniform = |binding: u32, dynamic: bool, size: u64| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
            min_binding_size: wgpu::BufferSize::new(size),
        },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("terrain"),
        entries: &[
            uniform(0, false, SCENE_BUFFER_BYTES),
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            uniform(2, true, PATCH_RECORD_BYTES),
            uniform(3, false, std::mem::size_of::<ModelUniforms>() as u64),
        ],
    })
}

/// The vertex layout of the shared grid: one `vec2f` sample offset at location 0.
pub(crate) const GRID_ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];

/// Grid vertex buffer layout for every terrain pipeline.
pub(crate) fn grid_buffer_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout { array_stride: 8, step_mode: wgpu::VertexStepMode::Vertex, attributes: &GRID_ATTRIBUTES }
}

/// Resident terrain resources and per-frame selection, drawn by any compatible terrain pipeline.
///
/// Borrows nothing after construction; the asset handle is shared. Dropping releases every owned GPU resource.
pub(crate) struct TerrainGeometry {
    asset: TerrainAsset,
    grid: TerrainGrid,
    selection: Vec<TerrainSelection>,
    stats: TerrainStats,
    patch_scratch: Vec<u8>,

    // Bind group first: it references the buffers and texture below.
    bind_group: wgpu::BindGroup,
    gpu_selection: Option<GpuSelection>,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    scene: wgpu::Buffer,
    patches: wgpu::Buffer,
    model: wgpu::Buffer,
    _height_view: wgpu::TextureView,
    _height_texture: wgpu::Texture,
}

impl TerrainGeometry {
    /// Upload the resident grid and heights and build the selection path for this device.
    pub(crate) fn new(
        gpu: &GpuContext,
        asset: &TerrainAsset,
        layout: &wgpu::BindGroupLayout,
    ) -> Result<Self, TerrainError> {
        let device = gpu.device();
        let manifest = asset.manifest();
        if manifest.size > device.limits().max_texture_dimension_2d {
            return Err(TerrainError::Unsupported(format!(
                "a {size}-sample heightfield exceeds the {limit}-texel texture limit",
                size = manifest.size,
                limit = device.limits().max_texture_dimension_2d
            )));
        }
        let patch_count = asset.patches().len();
        let grid = create_terrain_grid(manifest.tile_cells);
        let mut resident_gpu_bytes = 0;
        let mut buffer = |label: &str, contents: &[u8], usage: wgpu::BufferUsages| {
            resident_gpu_bytes += contents.len() as u64;
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage })
        };

        // Static topology and decoded world heights upload once.
        let vertices =
            buffer("terrain grid vertices", bytemuck::cast_slice(&grid.positions), wgpu::BufferUsages::VERTEX);
        let indices = buffer("terrain grid indices", bytemuck::cast_slice(&grid.indices), wgpu::BufferUsages::INDEX);
        let scene_usage = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let scene = buffer("terrain scene", &[0; SCENE_BUFFER_BYTES as usize], scene_usage);
        let identity = ModelUniforms {
            model: Mat4::IDENTITY.to_cols_array_2d(),
            previous_model: Mat4::IDENTITY.to_cols_array_2d(),
            flags: [1.0, 1.0, 0.0, 0.0],
        };
        let model = buffer("terrain model", bytemuck::bytes_of(&identity), scene_usage);

        // WebGPU-class backends write patch records from compute; WebGL 2 uploads them from the CPU.
        let use_gpu_selection = supports_gpu_selection(gpu);
        let patch_usage = if use_gpu_selection {
            scene_usage | wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC
        } else {
            scene_usage
        };
        let patch_bytes = vec![0; patch_count * PATCH_SLOT_BYTES as usize];
        let patches = buffer("terrain patch records", &patch_bytes, patch_usage);

        let gpu_selection = if use_gpu_selection {
            let (selection, bytes) = create_gpu_selection(device, asset, &grid, &scene, &patches);
            resident_gpu_bytes += bytes;
            Some(selection)
        } else {
            None
        };

        // Decode the global quantization once so every shader reads heights directly in world units; per-patch
        // rescaling would introduce seams.
        let heights: Vec<f32> = (0..asset.heights().len()).map(|index| asset.decoded_height(index) as f32).collect();
        let height_texture = device.create_texture_with_data(
            gpu.queue(),
            &wgpu::TextureDescriptor {
                label: Some("terrain heights"),
                size: wgpu::Extent3d { width: manifest.size, height: manifest.size, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bytemuck::cast_slice(&heights),
        );
        resident_gpu_bytes += (heights.len() * 4) as u64;
        let height_view = height_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: scene.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&height_view) },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &patches,
                        offset: 0,
                        size: wgpu::BufferSize::new(PATCH_RECORD_BYTES),
                    }),
                },
                wgpu::BindGroupEntry { binding: 3, resource: model.as_entire_binding() },
            ],
        });

        let tile_cells = manifest.tile_cells as usize;
        let max_triangles = patch_count * tile_cells * tile_cells * 2;
        let resident_cpu_bytes = (std::mem::size_of_val(asset.heights())
            + std::mem::size_of_val(grid.positions.as_slice())
            + std::mem::size_of_val(grid.indices.as_slice())) as u64;
        let stats = TerrainStats {
            patches: patch_count,
            triangles: 0,
            max_triangles,
            cpu_milliseconds: 0.0,
            resident_cpu_bytes,
            resident_gpu_bytes,
            frame_upload_bytes: SCENE_BUFFER_BYTES,
            triangle_count_kind: if use_gpu_selection {
                TerrainTriangleCount::UpperBound
            } else {
                TerrainTriangleCount::Selected
            },
        };

        Ok(Self {
            asset: asset.clone(),
            grid,
            selection: Vec::new(),
            stats,
            patch_scratch: patch_bytes,
            bind_group,
            gpu_selection,
            vertices,
            indices,
            scene,
            patches,
            model,
            _height_view: height_view,
            _height_texture: height_texture,
        })
    }

    /// The shared asset.
    pub(crate) fn asset(&self) -> &TerrainAsset {
        &self.asset
    }

    /// Statistics from the latest prepare.
    pub(crate) fn stats(&self) -> &TerrainStats {
        &self.stats
    }

    /// Upload the world transform and shading flags read by scene passes.
    pub(crate) fn write_model(&self, queue: &wgpu::Queue, model: &ModelUniforms) {
        queue.write_buffer(&self.model, 0, bytemuck::bytes_of(model));
    }

    /// Update selection for one frame. Records compute into `encoder` on the GPU path; never reads GPU data.
    ///
    /// `matrix` is the selection camera's view-projection; `view` uses the terrain's local space and CSS pixels. Fails
    /// before any upload or dispatch for invalid inputs.
    pub(crate) fn prepare(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        matrix: Mat4,
        view: &TerrainView,
        diagnostic: TerrainDiagnostic,
    ) -> Result<(), TerrainError> {
        validate_terrain_view(view)?;
        if !matrix.is_finite() {
            return Err(TerrainError::InvalidMatrix);
        }
        let start = Instant::now();
        let manifest = self.asset.manifest();

        let uniforms = SceneUniforms {
            matrix: matrix.to_cols_array_2d(),
            eye: [view.eye.x as f32, view.eye.y as f32, view.eye.z as f32, view.projection_scale as f32],
            geometry: [
                manifest.size as f32,
                manifest.tile_cells as f32,
                manifest.extent[0] as f32,
                manifest.extent[1] as f32,
            ],
            settings: [
                manifest.height_offset as f32,
                manifest.height_scale as f32,
                view.tolerance as f32,
                diagnostic.mode(),
            ],
            focus: [view.target.x as f32, view.target.y as f32, view.target.z as f32, view.target_radius as f32],
        };
        queue.write_buffer(&self.scene, 0, bytemuck::bytes_of(&uniforms));

        if let Some(selection) = &self.gpu_selection {
            // Separate ordered passes make every LOD exist before any invocation reads a neighbor.
            let workgroups = (self.stats.patches as u32).div_ceil(COMPUTE_WORKGROUP_SIZE);
            for pipeline in [&selection.evaluate, &selection.reconcile] {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("terrain selection"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &selection.bind_group, &[]);
                pass.dispatch_workgroups(workgroups, 1, 1);
            }
            self.stats.triangles = self.stats.max_triangles;
            self.stats.frame_upload_bytes = SCENE_BUFFER_BYTES;
        } else {
            // The CPU path applies the same selection and edge policy as compute.
            self.selection = select_terrain(&self.asset, view)?;
            self.stats.triangles = 0;
            for (index, (selection, patch)) in self.selection.iter().zip(self.asset.patches()).enumerate() {
                let record: [f32; 8] = [
                    patch.x as f32,
                    patch.z as f32,
                    selection.detail as f32,
                    0.0,
                    selection.edges[0] as f32,
                    selection.edges[1] as f32,
                    selection.edges[2] as f32,
                    selection.edges[3] as f32,
                ];
                let offset = index * PATCH_SLOT_BYTES as usize;
                self.patch_scratch[offset..offset + PATCH_RECORD_BYTES as usize]
                    .copy_from_slice(bytemuck::bytes_of(&record));
                self.stats.triangles += (self.grid.ranges[selection.lod].count / 3) as usize;
            }
            queue.write_buffer(&self.patches, 0, &self.patch_scratch);
            self.stats.frame_upload_bytes = SCENE_BUFFER_BYTES + self.patch_scratch.len() as u64;
        }

        self.stats.cpu_milliseconds = start.elapsed().as_secs_f64() * 1000.0;
        Ok(())
    }

    /// Draw every patch with `pipeline` into a borrowed pass, binding the terrain group at `group`; returns the draw
    /// count.
    ///
    /// The caller binds any groups the pipeline needs below `group`. The pass is not ended or submitted.
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, pipeline: &wgpu::RenderPipeline, group: u32) -> usize {
        pass.set_pipeline(pipeline);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);

        // The GPU-authored command chooses each patch's index range; the CPU path draws the selected range.
        let mut draws = 0;
        if let Some(selection) = &self.gpu_selection {
            for index in 0..self.stats.patches {
                pass.set_bind_group(group, &self.bind_group, &[(index as u64 * PATCH_SLOT_BYTES) as u32]);
                pass.draw_indexed_indirect(&selection.indirect, index as u64 * INDIRECT_COMMAND_BYTES);
                draws += 1;
            }
        } else {
            for (index, selection) in self.selection.iter().enumerate() {
                let range = self.grid.ranges[selection.lod];
                pass.set_bind_group(group, &self.bind_group, &[(index as u64 * PATCH_SLOT_BYTES) as u32]);
                pass.draw_indexed(range.first..range.first + range.count, 0, 0..1);
                draws += 1;
            }
        }
        draws
    }
}

/// Whether this device can select LODs with compute and draw indexed indirect commands.
fn supports_gpu_selection(gpu: &GpuContext) -> bool {
    let flags = gpu.adapter().get_downlevel_capabilities().flags;
    let limits = gpu.device().limits();
    gpu.capabilities().backend != crate::GpuBackend::Gl
        && flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS | wgpu::DownlevelFlags::INDIRECT_EXECUTION)
        && limits.max_storage_buffers_per_shader_stage >= 5
}

/// Build the compute selection resources; returns them with their resident byte count.
fn create_gpu_selection(
    device: &wgpu::Device,
    asset: &TerrainAsset,
    grid: &TerrainGrid,
    scene: &wgpu::Buffer,
    patches: &wgpu::Buffer,
) -> (GpuSelection, u64) {
    let patch_count = asset.patches().len();

    // Metadata packs the sample origin, world height bounds, and up to eight error levels into three vectors.
    let mut metadata = vec![0.0_f32; patch_count * (METADATA_BYTES as usize / 4)];
    for (index, patch) in asset.patches().iter().enumerate() {
        let record = &mut metadata[index * 12..(index + 1) * 12];
        record[0..4].copy_from_slice(&[patch.x as f32, patch.z as f32, patch.min as f32, patch.max as f32]);
        for (slot, error) in patch.errors.iter().take(ERROR_SLOTS).enumerate() {
            record[4 + slot] = *error as f32;
        }
    }
    let ranges: Vec<u32> = grid.ranges.iter().flat_map(|range| [range.first, range.count]).collect();
    let storage = wgpu::BufferUsages::STORAGE;
    let metadata_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("terrain metadata"),
        contents: bytemuck::cast_slice(&metadata),
        usage: storage,
    });
    let ranges_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("terrain ranges"),
        contents: bytemuck::cast_slice(&ranges),
        usage: storage,
    });
    let details = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("terrain details"),
        size: (patch_count * 4) as u64,
        usage: storage,
        mapped_at_creation: false,
    });
    let indirect = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("terrain indirect draws"),
        size: patch_count as u64 * INDIRECT_COMMAND_BYTES,
        usage: storage | wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    let storage_entry = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("terrain selection"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            storage_entry(1, true),
            storage_entry(2, false),
            storage_entry(3, false),
            storage_entry(4, false),
            storage_entry(5, true),
        ],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("terrain selection"),
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: scene.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: metadata_buffer.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: details.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: patches.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: indirect.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 5, resource: ranges_buffer.as_entire_binding() },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("terrain selection"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("terrain selection"),
        source: wgpu::ShaderSource::Wgsl(TERRAIN_SELECT_SOURCE.into()),
    });
    let compute = |entry_point: &str| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry_point),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some(entry_point),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    let bytes = (metadata.len() * 4 + ranges.len() * 4) as u64 + details.size() + indirect.size();
    let selection = GpuSelection {
        evaluate: compute("evaluate"),
        reconcile: compute("reconcile"),
        bind_group,
        indirect,
        _metadata: metadata_buffer,
        _details: details,
        _ranges: ranges_buffer,
    };
    (selection, bytes)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::BackendPreference;
    use crate::terrain::parse_terrain;
    use joy_core::math::DVec3;

    /// Copy a GPU buffer to the CPU, blocking until the queue finishes.
    fn read_buffer(gpu: &GpuContext, buffer: &wgpu::Buffer) -> Vec<u8> {
        let device = gpu.device();
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("terrain test readback"),
            size: buffer.size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, buffer.size());
        gpu.queue().submit([encoder.finish()]);
        staging.slice(..).map_async(wgpu::MapMode::Read, |result| result.expect("readback maps"));
        device.poll(wgpu::PollType::wait_indefinitely()).expect("device polls");
        let bytes = staging.slice(..).get_mapped_range().expect("mapped").to_vec();
        staging.unmap();
        bytes
    }

    /// The compute path writes the same records and draw ranges as CPU selection (the TypeScript browser parity check,
    /// whose largest descriptor difference was about 2e-6).
    #[test]
    fn compute_selection_matches_cpu_selection() {
        let gpu = match pollster::block_on(GpuContext::headless(BackendPreference::Auto)) {
            Ok(gpu) => gpu,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return;
            }
        };
        let asset = parse_terrain(
            include_str!("../../../../examples/terrain/assets/dragon-pit/terrain.json"),
            include_bytes!("../../../../examples/terrain/assets/dragon-pit/height.bin"),
        )
        .expect("the example asset is valid");
        let layout = terrain_bind_group_layout(gpu.device());
        let mut geometry = TerrainGeometry::new(&gpu, &asset, &layout).expect("resident terrain");
        if geometry.gpu_selection.is_none() {
            eprintln!("skipping compute parity: this adapter selects on the CPU");
            return;
        }
        let views = [
            TerrainView {
                eye: DVec3::new(1280.0, 1400.0, 1300.0),
                projection_scale: 965.7,
                tolerance: 2.0,
                target: DVec3::new(0.0, 226.0, 0.0),
                target_radius: 90.0,
            },
            TerrainView {
                eye: DVec3::new(-512.5, 320.0, 400.25),
                projection_scale: 1200.0,
                tolerance: 0.5,
                target: DVec3::new(-500.0, 300.0, 420.0),
                target_radius: 30.0,
            },
        ];
        for view in views {
            let mut encoder = gpu.device().create_command_encoder(&Default::default());
            geometry
                .prepare(gpu.queue(), &mut encoder, Mat4::IDENTITY, &view, TerrainDiagnostic::Material)
                .expect("valid frame");
            gpu.queue().submit([encoder.finish()]);
            let records = read_buffer(&gpu, &geometry.patches);
            let indirect = read_buffer(&gpu, &geometry.gpu_selection.as_ref().unwrap().indirect);
            let expected = select_terrain(&asset, &view).expect("valid view");

            let mut largest = 0.0_f64;
            for (index, (selection, patch)) in expected.iter().zip(asset.patches()).enumerate() {
                let offset = index * PATCH_SLOT_BYTES as usize;
                let record: [f32; 8] = bytemuck::pod_read_unaligned(&records[offset..offset + 32]);
                assert_eq!((record[0], record[1]), (patch.x as f32, patch.z as f32));
                let computed = [record[2], record[4], record[5], record[6], record[7]];
                let reference =
                    [selection.detail, selection.edges[0], selection.edges[1], selection.edges[2], selection.edges[3]];
                for (gpu_value, cpu_value) in computed.iter().zip(reference) {
                    largest = largest.max((f64::from(*gpu_value) - cpu_value).abs());
                }
                let command: [u32; 5] = bytemuck::pod_read_unaligned(&indirect[index * 20..index * 20 + 20]);
                let range = geometry.grid.ranges[record[2].floor() as usize];
                assert_eq!(command, [range.count, 1, range.first, 0, 0], "patch {index} draw");
            }
            assert!(largest < 1e-3, "largest descriptor difference {largest}");
        }
    }
}
