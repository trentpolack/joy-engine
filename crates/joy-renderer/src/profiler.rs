// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Frame profiling: how long each phase of a frame took to record on the CPU and, where the device supports timestamp
//! queries, to run on the GPU.
//!
//! The renderer marks the start of each phase as it records a frame. On the CPU a mark reads the clock. On the GPU it
//! is a compute pass that writes a timestamp as it begins, which WebGPU's pass-level timestamp rule allows as well as
//! native backends do. Each mark pass dispatches one thread of real work (`shaders/profile-mark.wgsl`), because Metal
//! samples a pass's counters only when its encoder executes something, and empty mark passes left every Apple GPU
//! timestamp at zero. A device whose timestamps still never advance falls back to CPU timings with one warning. At the end of the frame the timestamps resolve into a buffer that maps once the frame's
//! commands are submitted and finish, so GPU timings arrive a few frames late without ever stalling the CPU. WebGL 2 has
//! no timestamp queries; its profiles carry CPU timings only. Profiling is off until a host enables it and costs
//! nothing while off.

use std::sync::Arc;
use std::sync::atomic::{
    AtomicU8,
    Ordering,
};

use web_time::Instant;

/// One phase of a frame, in recording order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FramePhase {
    /// Culling, light selection, batching, and uploads before any pass.
    Prepare,
    /// Directional cascades and point-light faces.
    Shadows,
    /// Volumetric fog injection and integration.
    Volumetrics,
    /// The opaque depth pre-pass.
    DepthPrepass,
    /// Deferred surfaces writing the G-buffer.
    GBuffer,
    /// Deferred lighting: one full-screen pass per shading model.
    Lighting,
    /// Forward-shaded opaque surfaces and the sky (every opaque surface without deferred lighting).
    Scene,
    /// Volumetric fog over opaque surfaces.
    Fog,
    /// The back-to-front transparent queue.
    Transparency,
    /// Temporal antialiasing, motion blur, and adaptive exposure.
    Composite,
    /// Bloom and the display resolve.
    Post,
}

impl FramePhase {
    /// A short label for overlays.
    pub fn label(self) -> &'static str {
        match self {
            Self::Prepare => "prepare",
            Self::Shadows => "shadows",
            Self::Volumetrics => "volumetrics",
            Self::DepthPrepass => "depth pre-pass",
            Self::GBuffer => "g-buffer",
            Self::Lighting => "deferred lighting",
            Self::Scene => "scene",
            Self::Fog => "fog",
            Self::Transparency => "transparency",
            Self::Composite => "composite effects",
            Self::Post => "post-processing",
        }
    }
}

/// How long one phase took.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhaseTiming {
    /// Which phase.
    pub phase: FramePhase,
    /// Milliseconds spent recording the phase's commands.
    pub cpu_milliseconds: f64,
    /// Milliseconds the GPU spent running them, when the device supports timestamp queries.
    pub gpu_milliseconds: Option<f64>,
}

/// The timings of one frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameProfile {
    /// Which frame since profiling began these timings measured; GPU timings describe a frame a few frames old.
    pub frame: u64,
    /// The phases the frame ran, in order.
    pub phases: Vec<PhaseTiming>,
    /// Milliseconds spent recording the whole frame.
    pub cpu_milliseconds: f64,
    /// Milliseconds the GPU spent on the whole frame, when measured.
    pub gpu_milliseconds: Option<f64>,
}

/// Timestamps one frame can write: a mark per phase and one at the end.
const MAX_MARKS: u32 = 16;

/// Frames whose timestamps can be in flight at once. A frame that finds every slot busy goes unprofiled, so GPU-bound
/// hosts with deep frame queues still see GPU timings every few frames.
const READBACK_SLOTS: usize = 8;

/// Bytes per resolved timestamp.
const TIMESTAMP_BYTES: u64 = 8;

/// Consecutive frames whose GPU timestamps never advance before the profiler stops asking the device for them: a
/// driver that resolves every sample to the same value reports CPU timings instead of zeros.
const STALLED_FRAME_LIMIT: u32 = 8;

/// The mark pass's shader: one thread that counts marks, so every mark pass carries work.
const PROFILE_MARK_SOURCE: &str = include_str!("shaders/profile-mark.wgsl");

/// Readback states shared with a slot's map callback.
const MAP_PENDING: u8 = 0;
const MAP_READY: u8 = 1;
const MAP_FAILED: u8 = 2;

/// Profiling state for one renderer. Owned by the renderer and lent to each frame's recording.
#[derive(Default)]
pub(crate) struct FrameProfiler {
    enabled: bool,
    /// Frames begun since profiling was enabled.
    frame: u64,
    /// The frame being recorded.
    recording: Option<Recording>,
    /// GPU timestamps, when the device has timestamp queries; created on first enable.
    gpu: Option<GpuTimer>,
    /// The device's timestamps never advanced, so GPU timing stays off for this renderer.
    timestamps_unusable: bool,
    /// The latest finished profile.
    latest: Option<FrameProfile>,
}

/// The CPU side of the frame being recorded.
struct Recording {
    frame: u64,
    started: Instant,
    marks: Vec<(FramePhase, Instant)>,
    /// The readback slot whose query set this frame writes, if one was free.
    slot: Option<usize>,
}

/// Timestamp queries, their readback ring, and the work each mark pass carries.
struct GpuTimer {
    /// Nanoseconds per timestamp tick.
    period_nanoseconds: f64,
    slots: Vec<TimerSlot>,
    mark: MarkWork,
    /// Frames in a row whose timestamps all read the same value.
    stalled_frames: u32,
}

/// The one-thread dispatch inside every mark pass, and the counter it writes.
struct MarkWork {
    pipeline: wgpu::ComputePipeline,
    bind_group: wgpu::BindGroup,
    // Held for the bind group; nothing reads it.
    _counter: wgpu::Buffer,
}

/// One frame's query set and the buffers its timestamps travel through.
struct TimerSlot {
    query_set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    /// The frame waiting on this slot's timestamps, with its CPU timings filled in.
    waiting: Option<FrameProfile>,
    /// Timestamps the waiting frame wrote.
    written: u32,
    map_state: Arc<AtomicU8>,
}

impl FrameProfiler {
    /// Turn profiling on or off. Timestamp queries are used when `timestamps` (the device has them).
    pub(crate) fn set_enabled(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, enabled: bool, timestamps: bool) {
        self.enabled = enabled;
        self.recording = None;
        if !enabled {
            self.latest = None;
            return;
        }
        if timestamps && !self.timestamps_unusable && self.gpu.is_none() {
            self.gpu = Some(GpuTimer::new(device, queue));
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    /// The latest finished profile.
    pub(crate) fn latest(&self) -> Option<&FrameProfile> {
        self.latest.as_ref()
    }

    /// Collect finished GPU readbacks and start recording a frame.
    pub(crate) fn begin_frame(&mut self, device: &wgpu::Device) {
        if !self.enabled {
            return;
        }
        let mut slot = None;
        if let Some(gpu) = &mut self.gpu {
            // Native backends run map callbacks while polling; browsers run them from their event loop.
            let _ = device.poll(wgpu::PollType::Poll);
            for profile in gpu.collect() {
                self.latest = Some(profile);
            }
            slot = gpu.slots.iter().position(|slot| slot.waiting.is_none());
            if gpu.stalled_frames >= STALLED_FRAME_LIMIT {
                log::warn!(
                    "GPU timestamps on this device never advance; frame profiles show CPU timings only. Report the \
                     adapter if this persists."
                );
                self.gpu = None;
                self.timestamps_unusable = true;
                slot = None;
            }
        }
        self.recording = Some(Recording {
            frame: self.frame,
            started: Instant::now(),
            marks: Vec::new(),
            slot,
        });
        self.frame += 1;
    }

    /// Mark the start of `phase` in the frame being recorded.
    pub(crate) fn mark(&mut self, encoder: &mut wgpu::CommandEncoder, phase: FramePhase) {
        let Some(recording) = &mut self.recording else {
            return;
        };
        if recording.marks.len() as u32 + 1 >= MAX_MARKS {
            return;
        }
        if let (Some(gpu), Some(slot)) = (&self.gpu, recording.slot) {
            write_timestamp(encoder, &gpu.slots[slot].query_set, recording.marks.len() as u32, &gpu.mark);
        }
        recording.marks.push((phase, Instant::now()));
    }

    /// Finish the frame: mark its end, and resolve its timestamps for readback once `encoder` is submitted.
    pub(crate) fn end_frame(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let Some(recording) = self.recording.take() else {
            return;
        };
        let ended = Instant::now();
        let profile = cpu_profile(&recording, ended);
        let Some(gpu) = &mut self.gpu else {
            self.latest = Some(profile);
            return;
        };
        // Every readback slot was busy (the GPU is running frames behind): keep the latest GPU-timed profile rather than
        // replacing it with one that has no GPU timings, which would read as a device without timestamp queries exactly
        // when the GPU is the bottleneck.
        let Some(slot) = recording.slot else {
            return;
        };
        let written = recording.marks.len() as u32 + 1;
        let GpuTimer {
            slots,
            mark,
            ..
        } = gpu;
        slots[slot].record_end(encoder, written, profile, mark);
    }
}

impl GpuTimer {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let slots = (0..READBACK_SLOTS).map(|_| TimerSlot::new(device)).collect();
        Self {
            period_nanoseconds: f64::from(queue.get_timestamp_period()),
            slots,
            mark: MarkWork::new(device),
            stalled_frames: 0,
        }
    }

    /// Profiles whose timestamps have arrived; frees their slots and counts frames whose timestamps never advanced.
    fn collect(&mut self) -> Vec<FrameProfile> {
        let period = self.period_nanoseconds;
        let mut profiles = Vec::new();
        for slot in &mut self.slots {
            let Some((profile, advanced)) = slot.collect(period) else {
                continue;
            };
            self.stalled_frames = if advanced { 0 } else { self.stalled_frames + 1 };
            profiles.push(profile);
        }
        profiles
    }
}

impl MarkWork {
    fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("frame profile mark"),
            source: wgpu::ShaderSource::Wgsl(PROFILE_MARK_SOURCE.into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("frame profile mark"),
            layout: None,
            module: &module,
            entry_point: Some("mark_main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let counter = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame profile mark counter"),
            size: 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame profile mark"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: counter.as_entire_binding(),
            }],
        });
        Self {
            pipeline,
            bind_group,
            _counter: counter,
        }
    }
}

impl TimerSlot {
    fn new(device: &wgpu::Device) -> Self {
        let size = u64::from(MAX_MARKS) * TIMESTAMP_BYTES;
        Self {
            query_set: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("frame profile"),
                ty: wgpu::QueryType::Timestamp,
                count: MAX_MARKS,
            }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("frame profile resolve"),
                size,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            readback: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("frame profile readback"),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            waiting: None,
            written: 0,
            map_state: Arc::new(AtomicU8::new(MAP_PENDING)),
        }
    }

    /// Write the end timestamp, resolve the frame's timestamps, and map them for reading after submission.
    fn record_end(&mut self, encoder: &mut wgpu::CommandEncoder, written: u32, profile: FrameProfile, mark: &MarkWork) {
        write_timestamp(encoder, &self.query_set, written - 1, mark);
        encoder.resolve_query_set(&self.query_set, 0..written, &self.resolve, 0);
        let bytes = u64::from(written) * TIMESTAMP_BYTES;
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.readback, 0, Some(bytes));
        self.map_state.store(MAP_PENDING, Ordering::Release);
        let state = Arc::clone(&self.map_state);
        encoder.map_buffer_on_submit(&self.readback, wgpu::MapMode::Read, 0..bytes, move |result| {
            state.store(if result.is_ok() { MAP_READY } else { MAP_FAILED }, Ordering::Release);
        });
        self.waiting = Some(profile);
        self.written = written;
    }

    /// The waiting frame's profile with GPU timings once its readback has mapped, and whether its timestamps advanced.
    fn collect(&mut self, period_nanoseconds: f64) -> Option<(FrameProfile, bool)> {
        self.waiting.as_ref()?;
        match self.map_state.load(Ordering::Acquire) {
            MAP_READY => {}
            MAP_FAILED => {
                // The frame's commands never ran (a lost device or a dropped encoder); keep its CPU timings.
                self.map_state.store(MAP_PENDING, Ordering::Release);
                return self.waiting.take().map(|profile| (profile, true));
            }
            _ => return None,
        }
        let bytes = u64::from(self.written) * TIMESTAMP_BYTES;
        let ticks: Vec<u64> = match self.readback.slice(0..bytes).get_mapped_range() {
            Ok(range) => range
                .as_chunks::<{ TIMESTAMP_BYTES as usize }>()
                .0
                .iter()
                .map(|tick| u64::from_le_bytes(*tick))
                .collect(),
            Err(_) => Vec::new(),
        };
        self.readback.unmap();
        self.map_state.store(MAP_PENDING, Ordering::Release);
        let mut profile = self.waiting.take()?;
        let advanced = apply_gpu_timings(&mut profile, &ticks, period_nanoseconds);
        Some((profile, advanced))
    }
}

/// Write a timestamp at this point of the command stream: a compute pass that writes it as it begins and dispatches
/// one thread, so drivers that skip empty encoders (Metal) still sample it.
fn write_timestamp(encoder: &mut wgpu::CommandEncoder, query_set: &wgpu::QuerySet, index: u32, mark: &MarkWork) {
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("frame profile mark"),
        timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
            query_set,
            beginning_of_pass_write_index: Some(index),
            end_of_pass_write_index: None,
        }),
    });
    pass.set_pipeline(&mark.pipeline);
    pass.set_bind_group(0, &mark.bind_group, &[]);
    pass.dispatch_workgroups(1, 1, 1);
}

/// A recorded frame's CPU timings: each phase lasts until the next mark, the last until `ended`.
fn cpu_profile(recording: &Recording, ended: Instant) -> FrameProfile {
    let milliseconds = |from: Instant, to: Instant| to.saturating_duration_since(from).as_secs_f64() * 1000.0;
    let phases = recording
        .marks
        .iter()
        .enumerate()
        .map(|(index, (phase, started))| {
            let finished = recording.marks.get(index + 1).map_or(ended, |(_, next)| *next);
            PhaseTiming {
                phase: *phase,
                cpu_milliseconds: milliseconds(*started, finished),
                gpu_milliseconds: None,
            }
        })
        .collect();
    FrameProfile {
        frame: recording.frame,
        phases,
        cpu_milliseconds: milliseconds(recording.started, ended),
        gpu_milliseconds: None,
    }
}

/// Fill in GPU timings from a frame's resolved timestamps: each phase runs from its timestamp to the next, and a
/// timestamp written before the previous one counts as zero instead of wrapping. Returns whether the timestamps
/// advanced at all; when every timestamp reads the same value (a driver that never sampled them), the profile keeps
/// its CPU timings only rather than reporting zero GPU time.
fn apply_gpu_timings(profile: &mut FrameProfile, ticks: &[u64], period_nanoseconds: f64) -> bool {
    let advanced = ticks.windows(2).any(|pair| pair[0] != pair[1]);
    if !advanced {
        return false;
    }
    let milliseconds = |from: u64, to: u64| (to.saturating_sub(from) as f64 * period_nanoseconds) / 1.0e6;
    for (index, timing) in profile.phases.iter_mut().enumerate() {
        if let (Some(start), Some(end)) = (ticks.get(index), ticks.get(index + 1)) {
            timing.gpu_milliseconds = Some(milliseconds(*start, *end));
        }
    }
    if let (Some(first), Some(last)) = (ticks.first(), ticks.last()) {
        profile.gpu_milliseconds = Some(milliseconds(*first, *last));
    }
    true
}

#[cfg(test)]
#[path = "profiler.test.rs"]
mod tests;
