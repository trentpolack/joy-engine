// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Motion blur reconstructed from the scene's motion vectors (see `motion-blur.wgsl`): a tile-max pass, a
//! neighbor-max pass, and a gather pass at the scene size whose result is copied back into the scene color.

use bytemuck::{
    Pod,
    Zeroable,
};

use crate::pipelines::{
    ATTACHMENT_FORMAT,
    FULLSCREEN_SOURCE,
    FullscreenTarget,
    draw_fullscreen,
    fullscreen_pipeline,
    sampler_entry,
    texture_entry,
    uniform_entry,
};
use crate::scene_effects::config::MotionBlurOptions;
use crate::scene_effects::{
    EffectsFrame,
    copy_into_scene,
    effect_target,
    linear_sampler,
};

const MOTION_BLUR_SOURCE: &str = include_str!("shaders/motion-blur.wgsl");

/// Tile sizes in pixels. A tile is as wide as the longest blur's reach (half its length, since samples span the blur
/// both ways), so the neighbor-max pass sees every motion that can reach a pixel; the bounds keep the tile-max loop
/// cheap and the tile grid small.
const MIN_TILE_SIZE: u32 = 4;
const MAX_TILE_SIZE: u32 = 32;

/// Depth difference, in view units, over which a sample changes from in front of a pixel to behind it.
const SOFT_DEPTH_RANGE: f32 = 0.25;

/// `MotionBlur` in `motion-blur.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct MotionBlurUniforms {
    frame: [f32; 4],
    samples: [f32; 4],
    sky_reprojection: [[f32; 4]; 4],
}

/// Tile and output targets for one scene size and tile size.
struct BlurTargets {
    size: (u32, u32),
    tile_size: u32,
    tile_view: wgpu::TextureView,
    neighbor_view: wgpu::TextureView,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    /// Bind groups for the scene and motion views they were built against: one reading the neighbor-max tiles (for
    /// the tile-max and gather passes, which do not write them) and one reading the tile-max tiles.
    bound: Option<(wgpu::TextureView, wgpu::TextureView, BlurGroups)>,
}

#[derive(Clone)]
struct BlurGroups {
    reads_neighbors: wgpu::BindGroup,
    reads_tiles: wgpu::BindGroup,
}

/// Motion blur state for one renderer.
pub(crate) struct MotionBlur {
    layout: wgpu::BindGroupLayout,
    tile_max: wgpu::RenderPipeline,
    neighbor_max: wgpu::RenderPipeline,
    gather: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    format: wgpu::TextureFormat,
    targets: Option<BlurTargets>,
}

impl MotionBlur {
    pub(crate) fn new(device: &wgpu::Device, scene_format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("motion blur"),
            entries: &[uniform_entry(0), texture_entry(1), texture_entry(2), texture_entry(3), sampler_entry(4)],
        });
        let source = format!("{FULLSCREEN_SOURCE}\n{MOTION_BLUR_SOURCE}");
        let pipeline = |label: &str, entry: &str, format: wgpu::TextureFormat| {
            fullscreen_pipeline(device, label, &source, entry, &[&layout], format, wgpu::BlendState::REPLACE)
        };
        let tile_max = pipeline("motion blur tile max", "tileMaxMain", ATTACHMENT_FORMAT);
        let neighbor_max = pipeline("motion blur neighbor max", "neighborMaxMain", ATTACHMENT_FORMAT);
        let gather = pipeline("motion blur gather", "gatherMain", scene_format);
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("motion blur"),
            size: std::mem::size_of::<MotionBlurUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            layout,
            tile_max,
            neighbor_max,
            gather,
            sampler: linear_sampler(device, "motion blur"),
            uniforms,
            format: scene_format,
            targets: None,
        }
    }

    /// Blur the scene color along its motion.
    pub(crate) fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &EffectsFrame<'_>,
        motion: &wgpu::TextureView,
        options: &MotionBlurOptions,
    ) {
        // The longest blur in pixels, limited to what the tile reach can gather.
        let shorter_axis = frame.size.0.min(frame.size.1).max(1) as f32;
        let requested_pixels = options.max_length * shorter_axis;
        let tile_size = ((requested_pixels * 0.5).ceil() as u32).clamp(MIN_TILE_SIZE, MAX_TILE_SIZE);
        let max_pixels = requested_pixels.min((tile_size * 2) as f32);

        if self.targets.as_ref().is_none_or(|targets| targets.size != frame.size || targets.tile_size != tile_size) {
            self.targets = Some(self.allocate(device, frame.size, tile_size));
        }
        let uniforms = MotionBlurUniforms {
            frame: [frame.size.0 as f32, frame.size.1 as f32, options.shutter, max_pixels],
            samples: [options.sample_count as f32, tile_size as f32, SOFT_DEPTH_RANGE, 0.0],
            sky_reprojection: frame.sky_reprojection().to_cols_array_2d(),
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));

        let groups = self.bind(device, frame.scene_view, motion);
        let targets = self.targets.as_ref().expect("allocated above");
        let tile_target = FullscreenTarget::cleared(&targets.tile_view);
        draw_fullscreen(encoder, "motion blur tile max", tile_target, &self.tile_max, &[&groups.reads_neighbors]);
        let neighbor_target = FullscreenTarget::cleared(&targets.neighbor_view);
        draw_fullscreen(
            encoder,
            "motion blur neighbor max",
            neighbor_target,
            &self.neighbor_max,
            &[&groups.reads_tiles],
        );
        let output_target = FullscreenTarget::cleared(&targets.output_view);
        draw_fullscreen(encoder, "motion blur gather", output_target, &self.gather, &[&groups.reads_neighbors]);
        copy_into_scene(encoder, &targets.output, frame);
    }

    fn allocate(&self, device: &wgpu::Device, size: (u32, u32), tile_size: u32) -> BlurTargets {
        let tiles = (size.0.div_ceil(tile_size), size.1.div_ceil(tile_size));
        let tile_usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let tile = effect_target(device, "motion blur tiles", tiles, ATTACHMENT_FORMAT, tile_usage);
        let neighbor = effect_target(device, "motion blur neighbors", tiles, ATTACHMENT_FORMAT, tile_usage);
        let output_usage = tile_usage | wgpu::TextureUsages::COPY_SRC;
        let output = effect_target(device, "motion blur output", size, self.format, output_usage);
        BlurTargets {
            size,
            tile_size,
            tile_view: tile.create_view(&wgpu::TextureViewDescriptor::default()),
            neighbor_view: neighbor.create_view(&wgpu::TextureViewDescriptor::default()),
            output_view: output.create_view(&wgpu::TextureViewDescriptor::default()),
            output,
            bound: None,
        }
    }

    /// The passes' bind groups for these scene and motion views, rebuilt only when either changes.
    fn bind(&mut self, device: &wgpu::Device, scene: &wgpu::TextureView, motion: &wgpu::TextureView) -> BlurGroups {
        let targets = self.targets.as_mut().expect("allocated before binding");
        if let Some((bound_scene, bound_motion, groups)) = &targets.bound
            && bound_scene == scene
            && bound_motion == motion
        {
            return groups.clone();
        }
        let group = |tiles: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("motion blur"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(scene),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(motion),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(tiles),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            })
        };
        let groups = BlurGroups {
            reads_neighbors: group(&targets.neighbor_view),
            reads_tiles: group(&targets.tile_view),
        };
        targets.bound = Some((scene.clone(), motion.clone(), groups.clone()));
        groups
    }
}
