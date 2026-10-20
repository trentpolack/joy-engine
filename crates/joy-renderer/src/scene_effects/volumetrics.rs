// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Volumetric fog and lighting in a froxel volume aligned with the view.
//!
//! Each frame, before the scene pass: inject the medium and the light it scatters into every froxel, blended with the
//! previous frame's volume (`volumetric-inject.wgsl`), then integrate scattering and transmittance along every view
//! ray (`volumetric-integrate.wgsl`). After the opaque scene: blend the integrated fog over opaque surfaces and the sky
//! (`volumetric-apply.wgsl`). Transparent surfaces drawn afterwards read the integrated volume through the frame group
//! and fog themselves. Requires compute shaders.

use bytemuck::{
    Pod,
    Zeroable,
};

use crate::lighting::LightTextures;
use crate::pipelines::{
    FOG_VOLUME_FORMAT,
    FULLSCREEN_SOURCE,
    FullscreenTarget,
    draw_fullscreen,
    fullscreen_pipeline,
    sampler_entry,
    uniform_entry,
    volume_entry,
};
use crate::scene_effects::config::VolumetricOptions;
use crate::scene_effects::{
    EffectsFrame,
    linear_sampler,
};

const COMMON_SOURCE: &str = include_str!("shaders/volumetric-common.wgsl");
const INJECT_SOURCE: &str = include_str!("shaders/volumetric-inject.wgsl");
const INTEGRATE_SOURCE: &str = include_str!("shaders/volumetric-integrate.wgsl");
const APPLY_SOURCE: &str = include_str!("shaders/volumetric-apply.wgsl");

/// Most froxel columns or rows along the longer screen axis. Large drawing buffers widen the cells instead of growing
/// the volume, which keeps memory and injection work bounded (192 x 108 x 96 froxels at 1080p on high quality).
const MAX_GRID_EXTENT: u32 = 192;

/// Injection and integration workgroup size along X and Y (`@workgroup_size(8, 8, 1)` in both shaders).
const WORKGROUP_SIZE: u32 = 8;

/// The golden ratio's fractional part: stepping each frame's depth jitter by it spreads samples evenly through every
/// froxel's depth.
const SLICE_JITTER_STEP: f32 = 0.618_034;

/// `Volumetric` in `volumetric-common.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct VolumetricUniforms {
    inverse_projection: [[f32; 4]; 4],
    inverse_view: [[f32; 4]; 4],
    previous_view_projection: [[f32; 4]; 4],
    previous_view: [[f32; 4]; 4],
    grid: [f32; 4],
    medium: [f32; 4],
    albedo: [f32; 4],
    emissive: [f32; 4],
    intensity: [f32; 4],
}

/// `FogApply` in `volumetric-apply.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ApplyUniforms {
    inverse_projection: [[f32; 4]; 4],
    extent: [f32; 4],
}

/// A 3D texture and its view.
struct Volume {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// The volumes for one grid size: injected scattering ping-pong and the integrated result.
struct Volumes {
    grid: [u32; 3],
    /// Injection writes one scattering volume while reading the other as history; the groups below reach them.
    _scatter: [Volume; 2],
    integrated: Volume,
    /// Injection group `index` reads scattering `index` and writes the other; integration group `index` reads
    /// scattering `index`.
    inject_groups: [wgpu::BindGroup; 2],
    integrate_groups: [wgpu::BindGroup; 2],
    /// The scattering volume holding the latest frame.
    current: usize,
    /// The apply group for the depth view it was built against.
    apply_bound: Option<(wgpu::TextureView, wgpu::BindGroup)>,
}

/// Volumetric fog state for one renderer.
pub(crate) struct Volumetrics {
    frame_group: wgpu::BindGroup,
    inject_layout: wgpu::BindGroupLayout,
    integrate_layout: wgpu::BindGroupLayout,
    apply_layout: wgpu::BindGroupLayout,
    inject: wgpu::ComputePipeline,
    integrate: wgpu::ComputePipeline,
    apply: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    apply_uniforms: wgpu::Buffer,
    sampler: wgpu::Sampler,
    volumes: Option<Volumes>,
    /// Whether the current scattering volume holds a previous frame.
    valid: bool,
    frame_index: u32,
}

impl Volumetrics {
    /// Compile the passes. Injection reads the frame uniforms in `frame_buffer`, the frame's light clusters in `lights`,
    /// and the shadow receiver group (built with `shadow_layout`, visible to compute); the apply pass blends into
    /// `scene_format`.
    pub(crate) fn new(
        device: &wgpu::Device,
        frame_buffer: &wgpu::Buffer,
        lights: &LightTextures,
        shadow_layout: &wgpu::BindGroupLayout,
        scene_format: wgpu::TextureFormat,
    ) -> Self {
        let compute = |entry: wgpu::BindGroupLayoutEntry| wgpu::BindGroupLayoutEntry {
            visibility: wgpu::ShaderStages::COMPUTE,
            ..entry
        };
        let storage_volume = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: FOG_VOLUME_FORMAT,
                view_dimension: wgpu::TextureViewDimension::D3,
            },
            count: None,
        };
        // The frame uniforms and light textures at their scene-common.wgsl bindings; the fog volume bindings between them
        // are what this effect writes, so they stay unbound here.
        let mut frame_entries = vec![compute(uniform_entry(0))];
        frame_entries.extend(LightTextures::layout_entries(3, wgpu::ShaderStages::COMPUTE));
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("volumetric frame"),
            entries: &frame_entries,
        });
        let inject_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("volumetric inject"),
            entries: &[
                compute(uniform_entry(0)),
                compute(volume_entry(1)),
                compute(sampler_entry(2)),
                storage_volume(3),
            ],
        });
        let integrate_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("volumetric integrate"),
            entries: &[compute(uniform_entry(0)), compute(volume_entry(1)), storage_volume(2)],
        });
        let apply_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("volumetric apply"),
            entries: &[
                uniform_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                volume_entry(2),
                sampler_entry(3),
            ],
        });

        // Injection composes the scene contract for the frame uniforms, light data, and shadow lookups.
        let common = crate::pipelines::scene_common(device);
        let inject_source = format!("{common}\n{COMMON_SOURCE}\n{INJECT_SOURCE}");
        let inject = compute_pipeline(
            device,
            "volumetric inject",
            &inject_source,
            "injectMain",
            &[&frame_layout, shadow_layout, &inject_layout],
        );
        let integrate_source = format!("{COMMON_SOURCE}\n{INTEGRATE_SOURCE}");
        let integrate =
            compute_pipeline(device, "volumetric integrate", &integrate_source, "integrateMain", &[&integrate_layout]);

        // The fog adds its scattered light and attenuates the scene by its transmittance; scene alpha is kept.
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::SrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let apply_source = format!("{FULLSCREEN_SOURCE}\n{APPLY_SOURCE}");
        let apply = fullscreen_pipeline(
            device,
            "volumetric apply",
            &apply_source,
            "fragmentMain",
            &[&apply_layout],
            scene_format,
            blend,
        );

        let buffer = |label: &str, size: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let mut frame_bindings = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: frame_buffer.as_entire_binding(),
        }];
        frame_bindings.extend(lights.entries(3));
        let frame_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("volumetric frame"),
            layout: &frame_layout,
            entries: &frame_bindings,
        });
        Self {
            frame_group,
            inject_layout,
            integrate_layout,
            apply_layout,
            inject,
            integrate,
            apply,
            uniforms: buffer("volumetrics", std::mem::size_of::<VolumetricUniforms>()),
            apply_uniforms: buffer("volumetric apply", std::mem::size_of::<ApplyUniforms>()),
            sampler: linear_sampler(device, "volumetrics"),
            volumes: None,
            valid: false,
            frame_index: 0,
        }
    }

    /// Forget the previous frame's volume.
    pub(crate) fn reset(&mut self) {
        self.valid = false;
    }

    /// Size the volumes for a `size` render target before the frame's passes bind the integrated volume.
    pub(crate) fn prepare(&mut self, device: &wgpu::Device, size: (u32, u32), options: &VolumetricOptions) {
        let grid = froxel_grid(size, options);
        if self.volumes.as_ref().is_none_or(|volumes| volumes.grid != grid) {
            self.volumes = Some(self.allocate(device, grid));
            self.valid = false;
        }
    }

    /// The integrated volume the scene's transparent surfaces sample, after [`prepare`](Self::prepare).
    pub(crate) fn integrated_view(&self) -> Option<&wgpu::TextureView> {
        self.volumes.as_ref().map(|volumes| &volumes.integrated.view)
    }

    /// Inject and integrate this frame's volume. Runs after the shadow views and before any pass that samples the
    /// integrated volume; `shadows` is the shadow receiver group.
    pub(crate) fn encode_volume(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &EffectsFrame<'_>,
        options: &VolumetricOptions,
        shadows: &wgpu::BindGroup,
    ) {
        let Some(volumes) = &mut self.volumes else {
            return;
        };
        let valid = self.valid && !frame.reset_history;
        let jitter = if valid { (0.5 + (self.frame_index as f32 * SLICE_JITTER_STEP)).fract() } else { 0.5 };
        let camera = &frame.camera;
        let [albedo_red, albedo_green, albedo_blue] = options.albedo.rgb_array();
        let [emissive_red, emissive_green, emissive_blue] = options.emissive.rgb_array();
        let uniforms = VolumetricUniforms {
            inverse_projection: camera.projection.inverse().to_cols_array_2d(),
            inverse_view: camera.view.inverse().to_cols_array_2d(),
            previous_view_projection: frame.previous_view_projection.to_cols_array_2d(),
            previous_view: frame.previous_view.to_cols_array_2d(),
            grid: [volumes.grid[0] as f32, volumes.grid[1] as f32, volumes.grid[2] as f32, options.max_distance],
            medium: [options.density, options.height_falloff, options.base_height, options.anisotropy],
            albedo: [albedo_red, albedo_green, albedo_blue, if valid { options.history_weight } else { 0.0 }],
            emissive: [emissive_red, emissive_green, emissive_blue, jitter],
            intensity: [
                options.directional_intensity,
                options.point_light_intensity,
                options.ambient_intensity,
                if options.shadows { 1.0 } else { 0.0 },
            ],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));

        let groups_x = volumes.grid[0].div_ceil(WORKGROUP_SIZE);
        let groups_y = volumes.grid[1].div_ceil(WORKGROUP_SIZE);
        let next = 1 - volumes.current;
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("volumetrics"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.inject);
        pass.set_bind_group(0, &self.frame_group, &[]);
        pass.set_bind_group(1, shadows, &[]);
        pass.set_bind_group(2, &volumes.inject_groups[volumes.current], &[]);
        pass.dispatch_workgroups(groups_x, groups_y, volumes.grid[2]);
        pass.set_pipeline(&self.integrate);
        pass.set_bind_group(0, &volumes.integrate_groups[next], &[]);
        pass.dispatch_workgroups(groups_x, groups_y, 1);
        drop(pass);

        volumes.current = next;
        self.valid = true;
        self.frame_index = self.frame_index.wrapping_add(1);
    }

    /// Blend the integrated fog over the opaque scene and the sky, before transparency draws.
    pub(crate) fn encode_apply(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &EffectsFrame<'_>,
        options: &VolumetricOptions,
    ) {
        let uniforms = ApplyUniforms {
            inverse_projection: frame.raster_projection.inverse().to_cols_array_2d(),
            extent: [options.max_distance, 0.0, 0.0, 0.0],
        };
        queue.write_buffer(&self.apply_uniforms, 0, bytemuck::bytes_of(&uniforms));
        let Some(group) = self.apply_group(device, frame.depth_view) else {
            return;
        };
        draw_fullscreen(
            encoder,
            "volumetric apply",
            FullscreenTarget::blended(frame.scene_view),
            &self.apply,
            &[&group],
        );
    }

    fn allocate(&self, device: &wgpu::Device, grid: [u32; 3]) -> Volumes {
        let scatter = [0, 1].map(|index| volume(device, &format!("volumetric scattering {index}"), grid));
        let integrated = volume(device, "volumetric integrated", grid);
        let inject_groups = [0, 1].map(|history| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("volumetric inject"),
                layout: &self.inject_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&scatter[history].view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&scatter[1 - history].view),
                    },
                ],
            })
        });
        let integrate_groups = [0, 1].map(|source| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("volumetric integrate"),
                layout: &self.integrate_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniforms.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&scatter[source].view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&integrated.view),
                    },
                ],
            })
        });
        Volumes {
            grid,
            _scatter: scatter,
            integrated,
            inject_groups,
            integrate_groups,
            current: 0,
            apply_bound: None,
        }
    }

    /// The apply pass's group for this depth view, rebuilt only when it changes.
    fn apply_group(&mut self, device: &wgpu::Device, depth: &wgpu::TextureView) -> Option<wgpu::BindGroup> {
        let volumes = self.volumes.as_mut()?;
        if let Some((bound_depth, group)) = &volumes.apply_bound
            && bound_depth == depth
        {
            return Some(group.clone());
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("volumetric apply"),
            layout: &self.apply_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.apply_uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(depth),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&volumes.integrated.view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        volumes.apply_bound = Some((depth.clone(), group.clone()));
        Some(group)
    }
}

/// Froxel columns, rows, and depth slices for a `size` render target at the options' quality.
fn froxel_grid(size: (u32, u32), options: &VolumetricOptions) -> [u32; 3] {
    let (quality_cell, slices) = options.quality.grid();
    let longest = size.0.max(size.1).max(1);
    let cell = quality_cell.max(longest.div_ceil(MAX_GRID_EXTENT));
    [size.0.div_ceil(cell).max(1), size.1.div_ceil(cell).max(1), slices]
}

/// A froxel volume that compute passes write and later passes sample.
fn volume(device: &wgpu::Device, label: &str, grid: [u32; 3]) -> Volume {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: grid[0],
            height: grid[1],
            depth_or_array_layers: grid[2],
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: FOG_VOLUME_FORMAT,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    Volume {
        _texture: texture,
        view,
    }
}

fn compute_pipeline(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    entry: &str,
    layouts: &[&wgpu::BindGroupLayout],
) -> wgpu::ComputePipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layouts: Vec<Option<&wgpu::BindGroupLayout>> = layouts.iter().map(|layout| Some(*layout)).collect();
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &layouts,
        immediate_size: 0,
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
    })
}

#[cfg(test)]
#[path = "volumetrics.test.rs"]
mod tests;
