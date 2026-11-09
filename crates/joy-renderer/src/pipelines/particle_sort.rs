// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! GPU back-to-front ordering of particle instances for devices with compute shaders (`shaders/particle-sort.wgsl`).
//!
//! [`ParticleSorter::encode`] uploads one frame's instances in batch order, unsorted, with each instance's batch
//! (`segments`), then records compute dispatches into the frame's encoder: one key per instance (batch, view depth, and
//! upload position), a bitonic sort of the keys, and a scatter that writes the instances into the drawing buffer in
//! sorted order. Batches keep their ranges, source-over batches come out farthest first, and additive batches and equal
//! depths keep generation order, so the drawing buffer holds exactly what the CPU sort would have uploaded for the same
//! depths. WebGL 2 and adapters without compute keep the CPU sort ([`ParticlePass`](super::ParticlePass)).
//!
//! Buffers grow geometrically and persist between frames; bind groups are rebuilt only when a buffer is replaced.

use super::ParticleView;
use crate::gpu::GpuContext;
use crate::particles::ParticleInstance;
use bytemuck::{
    Pod,
    Zeroable,
};

/// The sort's compute kernels.
pub(crate) const PARTICLE_SORT_SOURCE: &str = include_str!("../shaders/particle-sort.wgsl");

/// The segment flag marking an additive batch, whose instances keep generation order (`UNSORTED_SEGMENT` in
/// particle-sort.wgsl).
pub(crate) const UNSORTED_SEGMENT: u32 = 1 << 31;

/// Threads per workgroup (`SORT_THREADS`).
const SORT_THREADS: u32 = 256;

/// Keys one workgroup sorts in workgroup memory (`SORT_BLOCK`); the padded key count is at least one block.
const SORT_BLOCK: u32 = 512;

/// Bytes between merge-stage parameter blocks: the largest uniform offset alignment WebGPU allows a device to require.
const STAGE_STRIDE: u64 = 256;

/// Bytes per sort key (`SortKey`).
const KEY_BYTES: u64 = 16;

/// Initial instance capacity of the sort's buffers; grows geometrically.
const INITIAL_CAPACITY: usize = 1024;

/// `SortView` in particle-sort.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct SortViewUniforms {
    eye: [f32; 4],
    forward: [f32; 4],
    counts: [u32; 4],
}

/// One compute dispatch of the sort, in recording order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SortDispatch {
    /// Sort every block of keys in workgroup memory.
    SortBlocks,
    /// One long-distance step of the merge of `width`-key sequences, at compare `distance` (at least one block).
    MergeStep { width: u32, distance: u32 },
    /// The in-block steps finishing the merge of `width`-key sequences.
    MergeBlocks { width: u32 },
}

/// The sort's compute pipelines, buffers, and bind groups. Borrows the device per call and the frame's encoder while
/// recording.
pub(crate) struct ParticleSorter {
    // Pipelines and the layouts their bind groups follow.
    keys_pipeline: wgpu::ComputePipeline,
    sort_blocks_pipeline: wgpu::ComputePipeline,
    merge_step_pipeline: wgpu::ComputePipeline,
    merge_blocks_pipeline: wgpu::ComputePipeline,
    scatter_pipeline: wgpu::ComputePipeline,
    buffers_layout: wgpu::BindGroupLayout,
    stage_layout: wgpu::BindGroupLayout,

    // The view block, merge-stage parameters, and per-instance buffers, each with its capacity.
    view_buffer: wgpu::Buffer,
    stage_buffer: wgpu::Buffer,
    stage_capacity: usize,
    source_buffer: wgpu::Buffer,
    segment_buffer: wgpu::Buffer,
    instance_capacity: usize,
    key_buffer: wgpu::Buffer,
    key_capacity: u32,

    // Bind groups for the current buffers, and the drawing buffer the scatter writes into.
    buffers_group: Option<(wgpu::Buffer, wgpu::BindGroup)>,
    stage_group: wgpu::BindGroup,

    // Scratch kept between frames: this frame's dispatches.
    dispatches: Vec<SortDispatch>,
}

impl ParticleSorter {
    /// Compile the sort kernels for `gpu`, which must support compute shaders.
    pub fn new(gpu: &GpuContext) -> Self {
        let device = gpu.device();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("particle sort"),
            source: wgpu::ShaderSource::Wgsl(PARTICLE_SORT_SOURCE.into()),
        });
        let buffers_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particle sort buffers"),
            entries: &[
                compute_buffer_entry(0, wgpu::BufferBindingType::Uniform),
                compute_buffer_entry(
                    1,
                    wgpu::BufferBindingType::Storage {
                        read_only: true,
                    },
                ),
                compute_buffer_entry(
                    2,
                    wgpu::BufferBindingType::Storage {
                        read_only: true,
                    },
                ),
                compute_buffer_entry(
                    3,
                    wgpu::BufferBindingType::Storage {
                        read_only: false,
                    },
                ),
                compute_buffer_entry(
                    4,
                    wgpu::BufferBindingType::Storage {
                        read_only: false,
                    },
                ),
            ],
        });
        let stage_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("particle sort stage"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(16),
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("particle sort"),
            bind_group_layouts: &[Some(&buffers_layout), Some(&stage_layout)],
            immediate_size: 0,
        });
        let pipeline = |entry_point: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry_point),
                layout: Some(&layout),
                module: &module,
                entry_point: Some(entry_point),
                compilation_options: Default::default(),
                cache: None,
            })
        };

        let view_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("particle sort view"),
            size: std::mem::size_of::<SortViewUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let stage_capacity = 16;
        let stage_buffer = stage_buffer(device, stage_capacity);
        let stage_group = stage_group(device, &stage_layout, &stage_buffer);
        let key_capacity = padded_count(INITIAL_CAPACITY);
        Self {
            keys_pipeline: pipeline("computeKeys"),
            sort_blocks_pipeline: pipeline("sortBlocks"),
            merge_step_pipeline: pipeline("mergeStep"),
            merge_blocks_pipeline: pipeline("mergeBlocks"),
            scatter_pipeline: pipeline("scatterInstances"),
            source_buffer: storage_buffer(device, "particle sort instances", instance_bytes(INITIAL_CAPACITY)),
            segment_buffer: storage_buffer(device, "particle sort segments", (INITIAL_CAPACITY as u64) * 4),
            instance_capacity: INITIAL_CAPACITY,
            key_buffer: storage_buffer(device, "particle sort keys", u64::from(key_capacity) * KEY_BYTES),
            key_capacity,
            buffers_layout,
            stage_layout,
            view_buffer,
            stage_buffer,
            stage_capacity,
            buffers_group: None,
            stage_group,
            dispatches: Vec::new(),
        }
    }

    /// Upload `instances` (batches contiguous, in batch order) and record their back-to-front order into
    /// `destination`, which must be a `STORAGE` buffer holding at least `instances.len()` instances.
    ///
    /// `segments` holds each instance's batch index, with [`UNSORTED_SEGMENT`] set for additive batches; batch indices
    /// must not decrease along the list. Depth is the distance along `view`'s forward axis in front of its eye, as the
    /// CPU sort measures it. Nothing is recorded for an empty list.
    pub fn encode(
        &mut self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        instances: &[ParticleInstance],
        segments: &[u32],
        view: &ParticleView,
        destination: &wgpu::Buffer,
    ) {
        debug_assert_eq!(instances.len(), segments.len(), "one segment per instance");
        if instances.is_empty() {
            return;
        }
        let count = instances.len() as u32;
        let padded = padded_count(instances.len());
        self.ensure_capacity(gpu.device(), instances.len(), padded);
        self.plan_dispatches(padded);
        self.ensure_stage_capacity(gpu.device());
        self.bind_destination(gpu.device(), destination);

        // Uploads land before the frame's command buffer runs.
        let queue = gpu.queue();
        queue.write_buffer(&self.source_buffer, 0, bytemuck::cast_slice(instances));
        queue.write_buffer(&self.segment_buffer, 0, bytemuck::cast_slice(segments));
        let uniforms = SortViewUniforms {
            eye: view.eye.extend(0.0).to_array(),
            forward: view.forward.extend(0.0).to_array(),
            counts: [count, padded, 0, 0],
        };
        queue.write_buffer(&self.view_buffer, 0, bytemuck::bytes_of(&uniforms));
        let mut stages = vec![0_u8; self.dispatches.len() * STAGE_STRIDE as usize];
        for (slot, dispatch) in self.dispatches.iter().enumerate() {
            let widths: [u32; 4] = match *dispatch {
                SortDispatch::SortBlocks => [0; 4],
                SortDispatch::MergeStep {
                    width,
                    distance,
                } => [width, distance, 0, 0],
                SortDispatch::MergeBlocks {
                    width,
                } => [width, 0, 0, 0],
            };
            let offset = slot * STAGE_STRIDE as usize;
            stages[offset..offset + 16].copy_from_slice(bytemuck::bytes_of(&widths));
        }
        queue.write_buffer(&self.stage_buffer, 0, &stages);

        // Keys, the sort, then the scatter into the drawing buffer; wgpu orders the dispatches' storage accesses.
        let Some((_, buffers_group)) = &self.buffers_group else {
            return;
        };
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("particle sort"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, buffers_group, &[]);
        pass.set_bind_group(1, &self.stage_group, &[0]);
        pass.set_pipeline(&self.keys_pipeline);
        pass.dispatch_workgroups(padded.div_ceil(SORT_THREADS), 1, 1);
        let blocks = padded / SORT_BLOCK;
        for (slot, dispatch) in self.dispatches.iter().enumerate() {
            pass.set_bind_group(1, &self.stage_group, &[(slot as u64 * STAGE_STRIDE) as u32]);
            match dispatch {
                SortDispatch::SortBlocks => pass.set_pipeline(&self.sort_blocks_pipeline),
                SortDispatch::MergeStep {
                    ..
                } => pass.set_pipeline(&self.merge_step_pipeline),
                SortDispatch::MergeBlocks {
                    ..
                } => pass.set_pipeline(&self.merge_blocks_pipeline),
            }
            // Every kernel but the key pass runs one thread per compare pair: half a block per workgroup.
            pass.dispatch_workgroups(blocks, 1, 1);
        }
        pass.set_bind_group(1, &self.stage_group, &[0]);
        pass.set_pipeline(&self.scatter_pipeline);
        pass.dispatch_workgroups(count.div_ceil(SORT_THREADS), 1, 1);
    }

    /// The dispatches that sort `padded` keys: every block in workgroup memory, then each wider merge's long-distance
    /// steps and its in-block finish.
    fn plan_dispatches(&mut self, padded: u32) {
        self.dispatches.clear();
        self.dispatches.push(SortDispatch::SortBlocks);
        let mut width = SORT_BLOCK * 2;
        while width <= padded {
            let mut distance = width / 2;
            while distance >= SORT_BLOCK {
                self.dispatches.push(SortDispatch::MergeStep {
                    width,
                    distance,
                });
                distance /= 2;
            }
            self.dispatches.push(SortDispatch::MergeBlocks {
                width,
            });
            width *= 2;
        }
    }

    /// Grow the per-instance and key buffers to hold `count` instances and `padded` keys.
    fn ensure_capacity(&mut self, device: &wgpu::Device, count: usize, padded: u32) {
        if count > self.instance_capacity {
            self.instance_capacity = count.next_power_of_two();
            self.source_buffer =
                storage_buffer(device, "particle sort instances", instance_bytes(self.instance_capacity));
            self.segment_buffer = storage_buffer(device, "particle sort segments", (self.instance_capacity as u64) * 4);
            self.buffers_group = None;
        }
        if padded > self.key_capacity {
            self.key_capacity = padded;
            self.key_buffer = storage_buffer(device, "particle sort keys", u64::from(padded) * KEY_BYTES);
            self.buffers_group = None;
        }
    }

    /// Grow the merge-stage parameters to one block per planned dispatch.
    fn ensure_stage_capacity(&mut self, device: &wgpu::Device) {
        if self.dispatches.len() <= self.stage_capacity {
            return;
        }
        self.stage_capacity = self.dispatches.len().next_power_of_two();
        self.stage_buffer = stage_buffer(device, self.stage_capacity);
        self.stage_group = stage_group(device, &self.stage_layout, &self.stage_buffer);
    }

    /// Bind the current buffers with `destination`, rebuilding the group only when either changed.
    fn bind_destination(&mut self, device: &wgpu::Device, destination: &wgpu::Buffer) {
        if self.buffers_group.as_ref().is_some_and(|(bound, _)| bound == destination) {
            return;
        }
        fn entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry {
                binding,
                resource: buffer.as_entire_binding(),
            }
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("particle sort buffers"),
            layout: &self.buffers_layout,
            entries: &[
                entry(0, &self.view_buffer),
                entry(1, &self.source_buffer),
                entry(2, &self.segment_buffer),
                entry(3, &self.key_buffer),
                entry(4, destination),
            ],
        });
        self.buffers_group = Some((destination.clone(), group));
    }
}

/// Keys sorted for `count` instances: the next power of two, at least one block.
fn padded_count(count: usize) -> u32 {
    (count.max(1).next_power_of_two() as u32).max(SORT_BLOCK)
}

/// Bytes of `count` instances.
fn instance_bytes(count: usize) -> u64 {
    (count * std::mem::size_of::<ParticleInstance>()) as u64
}

/// A compute-stage buffer binding of `ty`.
fn compute_buffer_entry(binding: u32, ty: wgpu::BufferBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// A storage buffer the queue writes.
fn storage_buffer(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Merge-stage parameters for `capacity` dispatches, one block every [`STAGE_STRIDE`] bytes.
fn stage_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("particle sort stages"),
        size: (capacity as u64) * STAGE_STRIDE,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// The stage group, read at a dynamic offset per dispatch.
fn stage_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, buffer: &wgpu::Buffer) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("particle sort stage"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer,
                offset: 0,
                size: wgpu::BufferSize::new(16),
            }),
        }],
    })
}

#[cfg(test)]
#[path = "particle_sort.test.rs"]
mod tests;
