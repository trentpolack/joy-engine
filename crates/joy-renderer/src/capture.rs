// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Offscreen presentation targets with CPU readback, for headless captures, golden images, and tests.

use crate::{
    GpuContext,
    RenderError,
};

/// Presentation format of offscreen captures. Non-sRGB, like presentation surfaces: captured bytes are the
/// sRGB-encoded display values a window shows.
pub const CAPTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Row alignment required for texture-to-buffer copies.
#[cfg(not(target_arch = "wasm32"))]
const COPY_ROW_ALIGNMENT: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

/// A renderable texture that can be read back as tightly packed RGBA8 rows.
pub struct OffscreenTarget {
    view: wgpu::TextureView,
    // Read back natively; browsers cannot block on readback, so the field is only kept alive there.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    texture: wgpu::Texture,
    width: u32,
    height: u32,
}

impl OffscreenTarget {
    /// Allocate a `width` by `height` capture target in [`CAPTURE_FORMAT`].
    pub fn new(gpu: &GpuContext, width: u32, height: u32) -> Result<Self, RenderError> {
        if width == 0 || height == 0 {
            return Err(RenderError::InvalidFrame("capture targets need a nonzero size".into()));
        }
        let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen capture"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: CAPTURE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Ok(Self { view, texture, width, height })
    }

    /// The view to pass as the renderer's output.
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// Target size in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Copy the target to the CPU as top-to-bottom RGBA8 rows with no padding. Blocks until the GPU finishes.
    ///
    /// Native only: browsers cannot block on GPU readback.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_rgba(&self, gpu: &GpuContext) -> Result<Vec<u8>, RenderError> {
        let unpadded_row = self.width * 4;
        let padded_row = unpadded_row.div_ceil(COPY_ROW_ALIGNMENT) * COPY_ROW_ALIGNMENT;
        let buffer = gpu.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture readback"),
            size: u64::from(padded_row) * u64::from(self.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder =
            gpu.device().create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("capture copy") });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
        );
        gpu.queue().submit([encoder.finish()]);

        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        gpu.device()
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| RenderError::Capture(error.to_string()))?;
        receiver
            .recv()
            .map_err(|error| RenderError::Capture(error.to_string()))?
            .map_err(|error| RenderError::Capture(error.to_string()))?;

        let mapped = slice.get_mapped_range().map_err(|error| RenderError::Capture(error.to_string()))?;
        let mut pixels = Vec::with_capacity((unpadded_row * self.height) as usize);
        for row in mapped.chunks(padded_row as usize) {
            pixels.extend_from_slice(&row[..unpadded_row as usize]);
        }
        drop(mapped);
        buffer.unmap();
        Ok(pixels)
    }
}
