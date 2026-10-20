// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/linear_composite.rs`. The composite only runs on extended linear (scRGB) surfaces, which test
//! machines rarely present, so these compile and run it offscreen.

use super::*;
use joy_renderer::{
    BackendPreference,
    GpuContext,
};

/// Half-float target the composite blends over, as an scRGB surface would be.
const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// One row of 32 half-float texels meets the copy alignment.
const WIDTH: u32 = 32;

#[test]
fn composite_shader_validates() {
    let module = wgpu::naga::front::wgsl::parse_str(COMPOSITE_SOURCE).unwrap_or_else(|error| panic!("{error}"));
    wgpu::naga::valid::Validator::new(Default::default(), wgpu::naga::valid::Capabilities::empty())
        .validate(&module)
        .unwrap_or_else(|error| panic!("{error:?}"));
}

/// UI white lands on the display's SDR white level, and translucent UI blends over the scene with premultiplied alpha.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn ui_lands_on_sdr_white_and_blends_over_the_scene() {
    let gpu = match pollster::block_on(GpuContext::headless(BackendPreference::Auto)) {
        Ok(gpu) => gpu,
        Err(error) => {
            eprintln!("skipping GPU test: {error}");
            return;
        }
    };
    let sdr_white_level = 2.5;
    let scene = wgpu::Color {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };

    // Opaque UI white over the scene shows at the SDR white level.
    let white = wgpu::Color {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
    let opaque = composite_over(&gpu, sdr_white_level, white, scene);
    assert!((opaque - sdr_white_level).abs() < 0.01, "opaque UI white is {opaque}");

    // Half-covered UI (premultiplied quarter-bright light) adds its scaled light to half the scene behind it.
    let translucent = wgpu::Color {
        r: 0.25,
        g: 0.25,
        b: 0.25,
        a: 0.5,
    };
    let blended = composite_over(&gpu, sdr_white_level, translucent, scene);
    let expected = (0.25 * sdr_white_level) + 0.5;
    assert!((blended - expected).abs() < 0.02, "translucent UI composites to {blended}, expected {expected}");
}

/// Clear the UI intermediate to `interface` (linear, premultiplied) and the output to `scene`, composite, and return the
/// output's red channel.
#[cfg(not(target_arch = "wasm32"))]
fn composite_over(gpu: &GpuContext, sdr_white_level: f32, interface: wgpu::Color, scene: wgpu::Color) -> f32 {
    let device = gpu.device();
    let mut composite = LinearComposite::new(device, gpu.queue(), OUTPUT_FORMAT, sdr_white_level);
    let output = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scRGB output"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OUTPUT_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: None,
    });
    clear(&mut encoder, composite.interface_view(device, [WIDTH, 1]), interface);
    clear(&mut encoder, &output_view, scene);
    composite.composite(&mut encoder, &output_view);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("scRGB readback"),
        size: u64::from(WIDTH * 8),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        output.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(WIDTH * 8),
                rows_per_image: Some(1),
            },
        },
        wgpu::Extent3d {
            width: WIDTH,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue().submit([encoder.finish()]);

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| result.expect("readback maps"));
    device.poll(wgpu::PollType::wait_indefinitely()).expect("readback completes");
    let mapped = slice.get_mapped_range().expect("mapped range");
    half_to_f32(u16::from_le_bytes([mapped[0], mapped[1]]))
}

#[cfg(not(target_arch = "wasm32"))]
fn clear(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, color: wgpu::Color) {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(color),
                store: wgpu::StoreOp::Store,
            },
            depth_slice: None,
        })],
        ..Default::default()
    });
}

/// Decode an IEEE 754 half-precision value.
#[cfg(not(target_arch = "wasm32"))]
fn half_to_f32(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = i32::from((bits >> 10) & 0x1f);
    let mantissa = f32::from(bits & 0x3ff);
    match exponent {
        0 => sign * mantissa * 2.0_f32.powi(-24),
        31 => sign * f32::INFINITY,
        _ => sign * (1.0 + (mantissa / 1024.0)) * 2.0_f32.powi(exponent - 15),
    }
}
