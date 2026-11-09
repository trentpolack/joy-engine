// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Buffer Overview: one screen showing the frame's buffers side by side (lit color, base color, normals, depth,
//! motion vectors, roughness, metallic, emissive, occlusion, lighting, specular color, and shading model), like a
//! visualization grid in a commercial engine.
//!
//! Surface attributes come from the same debug views every shading model reports, so the overview works for built-in
//! and registered models and for terrain. The renderer draws the lit frame first, with scene effects, so its depth and
//! motion targets hold the real frame's data, and visualizes those into their tiles. Every other tile redraws the
//! frame as one debug view with scene effects suspended, so the overview never disturbs temporal antialiasing or
//! exposure history. Each tile renders into an offscreen frame and is then scaled into its grid cell of the output.
//! The overview costs roughly one frame per tile, so it is a development view, not a presentation mode.

use crate::debug_view::DebugView;
use crate::pipelines::{
    FULLSCREEN_SOURCE,
    fullscreen_pipeline,
};

/// Grid columns.
pub const BUFFER_OVERVIEW_COLUMNS: u32 = 4;
/// Grid rows.
pub const BUFFER_OVERVIEW_ROWS: u32 = 3;
/// Gap between tiles, in output pixels at a scale factor of one.
pub const BUFFER_OVERVIEW_GAP: f32 = 4.0;

/// Depths beyond this view distance in meters show black on the depth tile's logarithmic ramp.
const DEPTH_DISPLAY_RANGE: f32 = 1000.0;

const OVERVIEW_SOURCE: &str = include_str!("shaders/buffer-overview.wgsl");

/// What one overview tile shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OverviewTile {
    /// The frame drawn as a debug view ([`DebugView::Lit`] is the finished frame).
    View(DebugView),
    /// Scene depth as linear view distance.
    Depth,
    /// Screen-space motion vectors (present only while scene effects that need them run).
    Motion,
}

impl OverviewTile {
    /// A short label for the tile.
    pub fn label(self) -> &'static str {
        match self {
            Self::View(DebugView::Lit) => "Final color",
            Self::View(view) => view.label(),
            Self::Depth => "Scene depth",
            Self::Motion => "Motion vectors",
        }
    }
}

/// The overview's tiles in reading order (left to right, top to bottom).
pub const BUFFER_OVERVIEW_TILES: [OverviewTile; 12] = [
    OverviewTile::View(DebugView::Lit),
    OverviewTile::View(DebugView::BaseColor),
    OverviewTile::View(DebugView::Normals),
    OverviewTile::Depth,
    OverviewTile::Motion,
    OverviewTile::View(DebugView::Roughness),
    OverviewTile::View(DebugView::Metallic),
    OverviewTile::View(DebugView::Emissive),
    OverviewTile::View(DebugView::AmbientOcclusion),
    OverviewTile::View(DebugView::LightingOnly),
    OverviewTile::View(DebugView::SpecularColor),
    OverviewTile::View(DebugView::ShadingModel),
];

/// A tile's rectangle inside an area: `[x, y, width, height]` in the area's units, origin top-left.
pub fn buffer_overview_tile_rect(index: usize, area_width: f32, area_height: f32, gap: f32) -> [f32; 4] {
    let columns = BUFFER_OVERVIEW_COLUMNS as f32;
    let rows = BUFFER_OVERVIEW_ROWS as f32;
    let width = ((area_width - (gap * (columns + 1.0))) / columns).max(1.0);
    let height = ((area_height - (gap * (rows + 1.0))) / rows).max(1.0);
    let column = (index as u32 % BUFFER_OVERVIEW_COLUMNS) as f32;
    let row = (index as u32 / BUFFER_OVERVIEW_COLUMNS) as f32;
    [gap + (column * (width + gap)), gap + (row * (height + gap)), width, height]
}

/// Uniform block of `buffer-overview.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct OverviewUniforms {
    depth_range: [f32; 4],
}

/// GPU resources of the overview: the offscreen frame each tile renders into, and the tile pipelines.
pub(crate) struct BufferOverview {
    // Views reference their textures; drop them first.
    frame_view: wgpu::TextureView,
    _frame_texture: wgpu::Texture,
    stand_in_motion_view: wgpu::TextureView,
    _stand_in_motion: wgpu::Texture,
    frame_size: (u32, u32),
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    color_pipeline: wgpu::RenderPipeline,
    depth_pipeline: wgpu::RenderPipeline,
    motion_pipeline: wgpu::RenderPipeline,
}

impl BufferOverview {
    /// Resources presenting into `format`, with an offscreen frame of `size` physical pixels.
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat, size: (u32, u32)) -> Self {
        let texture_entry = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("buffer overview"),
            entries: &[
                texture_entry(
                    0,
                    wgpu::TextureSampleType::Float {
                        filterable: true,
                    },
                ),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(2, wgpu::TextureSampleType::Depth),
                texture_entry(
                    3,
                    wgpu::TextureSampleType::Float {
                        filterable: false,
                    },
                ),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let source = format!("{FULLSCREEN_SOURCE}\n{OVERVIEW_SOURCE}");
        let pipeline = |entry: &str| {
            fullscreen_pipeline(
                device,
                &format!("buffer overview {entry}"),
                &source,
                entry,
                &[&layout],
                format,
                wgpu::BlendState::REPLACE,
            )
        };
        let (frame_texture, frame_view) = color_target(device, format, size);
        let stand_in_motion = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("buffer overview motion stand-in"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: crate::pipelines::ATTACHMENT_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        Self {
            frame_view,
            _frame_texture: frame_texture,
            stand_in_motion_view: stand_in_motion.create_view(&wgpu::TextureViewDescriptor::default()),
            _stand_in_motion: stand_in_motion,
            frame_size: size,
            format,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("buffer overview"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            uniforms: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("buffer overview"),
                size: std::mem::size_of::<OverviewUniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            color_pipeline: pipeline("colorTile"),
            depth_pipeline: pipeline("depthTile"),
            motion_pipeline: pipeline("motionTile"),
            layout,
        }
    }

    /// The offscreen frame tiles render into, reallocated when the output size changes.
    pub(crate) fn frame(&mut self, device: &wgpu::Device, size: (u32, u32)) -> &wgpu::TextureView {
        if self.frame_size != size {
            let (texture, view) = color_target(device, self.format, size);
            self._frame_texture = texture;
            self.frame_view = view;
            self.frame_size = size;
        }
        &self.frame_view
    }

    /// Record `tile`'s draw into cell `index` of `output` (`output_size` physical pixels, `gap` physical pixels), from the
    /// offscreen frame, `depth`, or `motion`. `clear` clears the output first.
    #[allow(clippy::too_many_arguments, reason = "one tile's sources, destination, and camera range")]
    pub(crate) fn draw_tile(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        tile: OverviewTile,
        index: usize,
        output: &wgpu::TextureView,
        output_size: (u32, u32),
        gap: f32,
        depth: &wgpu::TextureView,
        motion: Option<&wgpu::TextureView>,
        near_far: (f32, f32),
        clear: bool,
    ) {
        let uniforms = OverviewUniforms {
            depth_range: [near_far.0, near_far.1, DEPTH_DISPLAY_RANGE, 0.0],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("buffer overview tile"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.frame_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(depth),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(motion.unwrap_or(&self.stand_in_motion_view)),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: self.uniforms.as_entire_binding(),
                },
            ],
        });
        let pipeline = match tile {
            OverviewTile::View(_) => &self.color_pipeline,
            OverviewTile::Depth => &self.depth_pipeline,
            OverviewTile::Motion => &self.motion_pipeline,
        };
        let [x, y, width, height] = buffer_overview_tile_rect(index, output_size.0 as f32, output_size.1 as f32, gap);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("buffer overview tile"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if clear { wgpu::LoadOp::Clear(wgpu::Color::BLACK) } else { wgpu::LoadOp::Load },
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            ..Default::default()
        });
        pass.set_viewport(x, y, width, height, 0.0, 1.0);
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// An offscreen color target the frame renders into and the color tiles sample.
fn color_target(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    size: (u32, u32),
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("buffer overview frame"),
        size: wgpu::Extent3d {
            width: size.0.max(1),
            height: size.1.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

#[cfg(test)]
#[path = "buffer_overview.test.rs"]
mod tests;
