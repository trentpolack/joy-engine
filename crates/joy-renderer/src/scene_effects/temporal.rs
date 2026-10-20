// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Temporal antialiasing: two history targets the resolve alternates between, and the resolve pass that blends each
//! frame into them (see `temporal.wgsl`). The renderer jitters the raster projection; this pass reads the jittered
//! scene and writes the antialiased result back into the scene color.

use bytemuck::{
    Pod,
    Zeroable,
};

use crate::pipelines::{
    FULLSCREEN_SOURCE,
    FullscreenTarget,
    draw_fullscreen,
    fullscreen_pipeline,
    sampler_entry,
    texture_entry,
    uniform_entry,
};
use crate::scene_effects::config::TaaOptions;
use crate::scene_effects::{
    EffectsFrame,
    copy_into_scene,
    effect_target,
    linear_sampler,
};

const TEMPORAL_SOURCE: &str = include_str!("shaders/temporal.wgsl");

/// `Temporal` in `temporal.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct TemporalUniforms {
    frame: [f32; 4],
    sky_reprojection: [[f32; 4]; 4],
}

/// History targets for one scene size, and the resolve's bind groups for the scene and motion views they were built
/// against. Group `index` reads history `index` and writes the other one.
struct History {
    size: (u32, u32),
    textures: [wgpu::Texture; 2],
    views: [wgpu::TextureView; 2],
    current: usize,
    bound: Option<(wgpu::TextureView, wgpu::TextureView, [wgpu::BindGroup; 2])>,
}

/// Temporal antialiasing state for one renderer.
pub(crate) struct TemporalAntialiasing {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    format: wgpu::TextureFormat,
    history: Option<History>,
    /// Whether the current history holds a previous frame.
    valid: bool,
}

impl TemporalAntialiasing {
    pub(crate) fn new(device: &wgpu::Device, scene_format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("temporal antialiasing"),
            entries: &[uniform_entry(0), texture_entry(1), texture_entry(2), texture_entry(3), sampler_entry(4)],
        });
        let source = format!("{FULLSCREEN_SOURCE}\n{TEMPORAL_SOURCE}");
        let pipeline = fullscreen_pipeline(
            device,
            "temporal antialiasing",
            &source,
            "fragmentMain",
            &[&layout],
            scene_format,
            wgpu::BlendState::REPLACE,
        );
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("temporal antialiasing"),
            size: std::mem::size_of::<TemporalUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            layout,
            pipeline,
            sampler: linear_sampler(device, "temporal antialiasing"),
            uniforms,
            format: scene_format,
            history: None,
            valid: false,
        }
    }

    /// Forget the history, so the next frame starts fresh.
    pub(crate) fn reset(&mut self) {
        self.valid = false;
    }

    /// Resolve this frame against its history and write the result into the scene color.
    pub(crate) fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &EffectsFrame<'_>,
        motion: &wgpu::TextureView,
        options: &TaaOptions,
    ) {
        if self.history.as_ref().is_none_or(|history| history.size != frame.size) {
            self.history = Some(self.allocate(device, frame.size));
            self.valid = false;
        }
        let valid = self.valid && !frame.reset_history;
        let uniforms = TemporalUniforms {
            frame: [
                1.0 / frame.size.0 as f32,
                1.0 / frame.size.1 as f32,
                options.history_weight,
                if valid { 1.0 } else { 0.0 },
            ],
            sky_reprojection: frame.sky_reprojection().to_cols_array_2d(),
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));

        let groups = self.bind(device, frame.scene_view, motion);
        let history = self.history.as_mut().expect("allocated above");
        let next = 1 - history.current;
        let target = FullscreenTarget::cleared(&history.views[next]);
        draw_fullscreen(encoder, "temporal antialiasing", target, &self.pipeline, &[&groups[history.current]]);
        copy_into_scene(encoder, &history.textures[next], frame);
        history.current = next;
        self.valid = true;
    }

    fn allocate(&self, device: &wgpu::Device, size: (u32, u32)) -> History {
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC;
        let textures =
            [0, 1].map(|index| effect_target(device, &format!("taa history {index}"), size, self.format, usage));
        let views = [0, 1].map(|index| textures[index].create_view(&wgpu::TextureViewDescriptor::default()));
        History {
            size,
            textures,
            views,
            current: 0,
            bound: None,
        }
    }

    /// The resolve's bind groups for these scene and motion views, rebuilt only when either changes.
    fn bind(
        &mut self,
        device: &wgpu::Device,
        scene: &wgpu::TextureView,
        motion: &wgpu::TextureView,
    ) -> [wgpu::BindGroup; 2] {
        let history = self.history.as_mut().expect("allocated before binding");
        if let Some((bound_scene, bound_motion, groups)) = &history.bound
            && bound_scene == scene
            && bound_motion == motion
        {
            return groups.clone();
        }
        let groups = [0, 1].map(|read| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("temporal antialiasing"),
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
                        resource: wgpu::BindingResource::TextureView(&history.views[read]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(motion),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            })
        });
        history.bound = Some((scene.clone(), motion.clone(), groups.clone()));
        groups
    }
}
