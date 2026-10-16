// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Optional, non-blocking timestamp profiling of the standalone terrain render pass.

use std::sync::{Arc, Mutex};

/// Timestamps per sample: the beginning and end of the render pass.
const TIMESTAMP_QUERY_COUNT: u32 = 2;
/// Bytes of two resolved 64-bit timestamps.
const TIMESTAMP_BYTES: u64 = 16;
/// Minimum interval between samples.
const SAMPLE_INTERVAL_MILLISECONDS: f64 = 500.0;

/// What the latest timing sample reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerrainTimingStatus {
    /// The device supports timestamps and no sample has completed yet.
    Awaiting,
    /// The latest sample completed; it measures the render pass and excludes compute selection.
    Complete,
    /// The device has no timestamp queries.
    Unsupported,
    /// Reading the latest sample failed.
    Failed(String),
}

impl std::fmt::Display for TerrainTimingStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Awaiting => formatter.write_str("awaiting timestamp sample"),
            Self::Complete => formatter.write_str("render pass (compute excluded)"),
            Self::Unsupported => formatter.write_str("unsupported"),
            Self::Failed(error) => write!(formatter, "timestamp read failed: {error}"),
        }
    }
}

/// Where the single reusable sample is in its lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SampleState {
    Idle,
    /// Timestamp writes were handed to a pass that has not been resolved yet.
    Active,
    /// The resolve and copy are recorded; the caller submits them before the next poll.
    Recorded,
    /// The readback buffer is mapping.
    Mapping,
}

/// GPU query resources, present only when the device supports timestamp queries.
struct TimingQueries {
    query_set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    period_nanoseconds: f64,
}

/// Timestamp profiling for one render pass at a time.
///
/// One query pair is reused, at most one readback is pending, and no frame waits for a result. Sequence each sample as
/// [`begin`](Self::begin) (pass timestamp writes), [`resolve`](Self::resolve) after the pass, submit, then
/// [`poll`](Self::poll) on a later frame.
pub struct TerrainTiming {
    queries: Option<TimingQueries>,
    state: SampleState,
    last_sample_milliseconds: f64,
    mapped: Arc<Mutex<Option<Result<(), String>>>>,
    milliseconds: Option<f64>,
    status: TerrainTimingStatus,
}

impl TerrainTiming {
    /// Create timing resources when the device enabled `TIMESTAMP_QUERY`; otherwise report unsupported.
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let queries = device.features().contains(wgpu::Features::TIMESTAMP_QUERY).then(|| TimingQueries {
            query_set: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("terrain timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: TIMESTAMP_QUERY_COUNT,
            }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("terrain timestamp resolve"),
                size: TIMESTAMP_BYTES,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            readback: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("terrain timestamp readback"),
                size: TIMESTAMP_BYTES,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            period_nanoseconds: f64::from(queue.get_timestamp_period()),
        });
        let status = if queries.is_some() { TerrainTimingStatus::Awaiting } else { TerrainTimingStatus::Unsupported };
        Self {
            queries,
            state: SampleState::Idle,
            last_sample_milliseconds: f64::NEG_INFINITY,
            mapped: Arc::default(),
            milliseconds: None,
            status,
        }
    }

    /// Duration of the latest completed sample in milliseconds.
    pub fn milliseconds(&self) -> Option<f64> {
        self.milliseconds
    }

    /// Status of the latest sample.
    pub fn status(&self) -> &TerrainTimingStatus {
        &self.status
    }

    /// Reserve a sample for the caller's next render pass; `now_milliseconds` is a monotonic frame time.
    ///
    /// Returns `None` while another sample is active or pending, inside the sampling interval, or when unsupported.
    pub fn begin(&mut self, now_milliseconds: f64) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        let queries = self.queries.as_ref()?;
        let inside_interval = (now_milliseconds - self.last_sample_milliseconds) < SAMPLE_INTERVAL_MILLISECONDS;
        if self.state != SampleState::Idle || inside_interval {
            return None;
        }
        self.state = SampleState::Active;
        self.last_sample_milliseconds = now_milliseconds;
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &queries.query_set,
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: Some(1),
        })
    }

    /// Record the resolve and readback copy for the active sample after its pass ends. The caller submits `encoder`.
    pub fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let Some(queries) = self.queries.as_ref() else {
            return;
        };
        if self.state != SampleState::Active {
            return;
        }
        encoder.resolve_query_set(&queries.query_set, 0..TIMESTAMP_QUERY_COUNT, &queries.resolve, 0);
        encoder.copy_buffer_to_buffer(&queries.resolve, 0, &queries.readback, 0, TIMESTAMP_BYTES);
        self.state = SampleState::Recorded;
    }

    /// Advance a submitted sample without blocking: request the readback, or read a completed one.
    ///
    /// Call only after the encoder passed to [`resolve`](Self::resolve) was submitted (for example, on the next frame).
    pub fn poll(&mut self, device: &wgpu::Device) {
        let Some(queries) = self.queries.as_ref() else {
            return;
        };
        match self.state {
            SampleState::Recorded => {
                let slot = self.mapped.clone();
                queries.readback.slice(..).map_async(wgpu::MapMode::Read, move |result| {
                    if let Ok(mut slot) = slot.lock() {
                        *slot = Some(result.map_err(|error| error.to_string()));
                    }
                });
                self.state = SampleState::Mapping;
            }
            SampleState::Mapping => {
                // Natively, callbacks run when the device is polled; browsers deliver them on their own.
                let _ = device.poll(wgpu::PollType::Poll);
                let result = self.mapped.lock().ok().and_then(|mut slot| slot.take());
                match result {
                    Some(Ok(())) => {
                        self.read_mapped();
                        self.state = SampleState::Idle;
                    }
                    Some(Err(error)) => {
                        self.status = TerrainTimingStatus::Failed(error);
                        self.state = SampleState::Idle;
                    }
                    None => {}
                }
            }
            SampleState::Idle | SampleState::Active => {}
        }
    }

    /// Convert the mapped timestamp pair into milliseconds and release the mapping.
    fn read_mapped(&mut self) {
        let Some(queries) = self.queries.as_ref() else {
            return;
        };
        let duration = match queries.readback.slice(..).get_mapped_range() {
            Ok(range) => {
                let begin: u64 = bytemuck::pod_read_unaligned(&range[0..8]);
                let end: u64 = bytemuck::pod_read_unaligned(&range[8..16]);
                end.checked_sub(begin)
            }
            Err(error) => {
                self.status = TerrainTimingStatus::Failed(error.to_string());
                return;
            }
        };
        queries.readback.unmap();
        match duration {
            Some(ticks) => {
                self.milliseconds = Some((ticks as f64 * queries.period_nanoseconds) / 1_000_000.0);
                self.status = TerrainTimingStatus::Complete;
            }
            None => self.status = TerrainTimingStatus::Failed("timestamps ran backward".into()),
        }
    }
}
