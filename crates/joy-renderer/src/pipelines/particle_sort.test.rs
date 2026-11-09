// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/pipelines/particle_sort.rs`.

use joy_core::math::{
    Mat4,
    Vec3,
};
use joy_core::{
    SeededRandom,
    random_range,
};

use super::*;
use crate::gpu::BackendPreference;
use crate::pipelines::ParticleView;
use crate::pipelines::particles::order_back_to_front;

/// One test batch: its instance count and whether it is additive (kept in generation order).
const BATCHES: [(usize, bool); 4] = [(700, false), (50, true), (1, false), (600, false)];

/// A device with compute shaders, or `None` (with the reason printed) to skip.
fn compute_gpu() -> Option<GpuContext> {
    match pollster::block_on(GpuContext::headless(BackendPreference::Auto)) {
        Ok(gpu) if gpu.capabilities().compute => Some(gpu),
        Ok(_) => {
            eprintln!("skipping GPU sort test: the adapter has no compute shaders");
            None
        }
        Err(error) => {
            eprintln!("skipping GPU sort test: {error}");
            None
        }
    }
}

/// Instances scattered through a box in front of the camera, each with a distinct tint so reordering is visible in
/// the bytes; every seventh repeats an earlier position, so equal depths must keep generation order.
fn scattered_instances(count: usize, random: &mut SeededRandom) -> Vec<ParticleInstance> {
    let mut instances: Vec<ParticleInstance> = Vec::with_capacity(count);
    for index in 0..count {
        let position = if index % 7 == 6 {
            instances[index - 3].position()
        } else {
            let mut coordinate = || random_range(-20.0, 20.0, random) as f32;
            Vec3::new(coordinate(), coordinate(), coordinate())
        };
        instances.push(ParticleInstance {
            position_size: position.extend(1.0).to_array(),
            color: [index as f32, 0.5, 0.25, 1.0],
            shape: index as u32,
            ..ParticleInstance::default()
        });
    }
    instances
}

/// Read `buffer`'s first `count` instances back from the GPU.
fn read_instances(gpu: &GpuContext, buffer: &wgpu::Buffer, count: usize) -> Vec<ParticleInstance> {
    let size = instance_bytes(count);
    let staging = gpu.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("sorted readback"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device().create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
    gpu.queue().submit([encoder.finish()]);
    let slice = staging.slice(..);
    let (sender, receiver) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    gpu.device().poll(wgpu::PollType::wait_indefinitely()).expect("device polls");
    receiver.recv().expect("map callback").expect("buffer maps");
    let mapped = slice.get_mapped_range().expect("mapped range");
    let instances = bytemuck::cast_slice::<u8, ParticleInstance>(&mapped).to_vec();
    drop(mapped);
    staging.unmap();
    instances
}

/// The GPU sort fills the drawing buffer with exactly what the CPU fallback uploads: batches keep their ranges, each
/// source-over batch back to front with ties in generation order, and additive batches untouched. The batches span
/// several sort blocks, so the in-block sort, the cross-block merge steps, and the in-block merges all run.
#[test]
fn gpu_sort_matches_the_cpu_order() {
    let Some(gpu) = compute_gpu() else {
        return;
    };
    let view = ParticleView {
        view_projection: Mat4::IDENTITY,
        eye: Vec3::new(3.0, 4.0, 60.0),
        forward: Vec3::new(0.1, -0.2, -1.0).normalize(),
    };

    // Lay out the batches as the particle pass does: unsorted with segments for the GPU, sorted for the CPU fallback.
    let mut random = SeededRandom::new(42);
    let (mut unsorted, mut segments, mut expected, mut order) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (batch, (count, additive)) in BATCHES.into_iter().enumerate() {
        let instances = scattered_instances(count, &mut random);
        unsorted.extend_from_slice(&instances);
        let segment = (batch as u32) | if additive { UNSORTED_SEGMENT } else { 0 };
        segments.extend(std::iter::repeat_n(segment, count));
        if additive {
            expected.extend_from_slice(&instances);
        } else {
            order_back_to_front(&instances, &view, &mut order, &mut expected);
        }
    }

    let destination = gpu.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("sorted instances"),
        size: instance_bytes(unsorted.len()),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let mut sorter = ParticleSorter::new(&gpu);
    let mut encoder = gpu.device().create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    sorter.encode(&gpu, &mut encoder, &unsorted, &segments, &view, &destination);
    gpu.queue().submit([encoder.finish()]);

    let sorted = read_instances(&gpu, &destination, unsorted.len());
    let first_mismatch = sorted.iter().zip(&expected).position(|(gpu, cpu)| gpu != cpu);
    assert_eq!(first_mismatch, None, "GPU and CPU orders differ");
    assert_eq!(sorted.len(), expected.len());
}
