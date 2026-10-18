// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! GPU copies of PBR material maps and their bind groups.
//!
//! Each [`MaterialTextures`] set gets one bind group (group 3 of the PBR pipelines, group 1 of the masked caster) the
//! first time a frame draws it; images upload once and are shared between sets. Missing maps bind white (or a flat
//! normal), so untextured PBR surfaces use one shared default group. Images upload unchanged as UNORM: the shader
//! decodes color maps from sRGB and reads data maps linearly.

use std::collections::HashMap;
use std::sync::Weak;

use wgpu::util::DeviceExt;

use crate::RenderError;
use crate::mesh::MaterialTextures;
use crate::texture::TextureAsset;

/// An uploaded image, released once every handle to its asset drops.
struct UploadedTexture {
    view: wgpu::TextureView,
    _texture: wgpu::Texture,
    asset: Weak<dyn std::any::Any + Send + Sync>,
}

/// A material's bind group, released once every handle to the material drops.
struct MaterialGroup {
    group: wgpu::BindGroup,
    _flags: wgpu::Buffer,
    material: Weak<dyn std::any::Any + Send + Sync>,
}

/// Uploaded material maps and bind groups.
pub(crate) struct MaterialMaps {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    default_group: wgpu::BindGroup,
    white: (wgpu::Texture, wgpu::TextureView),
    flat_normal: (wgpu::Texture, wgpu::TextureView),
    textures: HashMap<u64, UploadedTexture>,
    groups: HashMap<u64, MaterialGroup>,
}

impl MaterialMaps {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("material maps"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white = solid(device, queue, "material white", [255, 255, 255, 255]);
        let flat_normal = solid(device, queue, "material flat normal", [128, 128, 255, 255]);
        let views = [&white.1, &flat_normal.1, &white.1, &white.1, &white.1, &white.1];
        let default_group = group(device, layout, &sampler, views, false).0;
        Self {
            layout: layout.clone(),
            sampler,
            default_group,
            white,
            flat_normal,
            textures: HashMap::new(),
            groups: HashMap::new(),
        }
    }

    /// The bind group for a material, or the white default.
    pub fn group(&self, material: Option<u64>) -> &wgpu::BindGroup {
        material.and_then(|id| self.groups.get(&id)).map_or(&self.default_group, |entry| &entry.group)
    }

    /// Upload a material's maps and build its bind group the first time it is drawn.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        material: &MaterialTextures,
        max_texture_dimension: u32,
    ) -> Result<(), RenderError> {
        if self.groups.contains_key(&material.id()) {
            return Ok(());
        }
        let maps = material.maps();
        let sources = [&maps.base_color, &maps.normal, &maps.emissive, &maps.metallic, &maps.roughness, &maps.ao];
        for texture in sources.iter().copied().flatten() {
            self.upload(device, queue, texture, max_texture_dimension)?;
        }
        let view = |texture: &Option<TextureAsset>, fallback: &'_ wgpu::TextureView| -> wgpu::TextureView {
            texture
                .as_ref()
                .and_then(|texture| self.textures.get(&texture.id()))
                .map_or(fallback, |uploaded| &uploaded.view)
                .clone()
        };
        let views = [
            view(&maps.base_color, &self.white.1),
            view(&maps.normal, &self.flat_normal.1),
            view(&maps.emissive, &self.white.1),
            view(&maps.metallic, &self.white.1),
            view(&maps.roughness, &self.white.1),
            view(&maps.ao, &self.white.1),
        ];
        let (group, flags) = group(device, &self.layout, &self.sampler, views.each_ref(), maps.normal.is_some());
        self.groups.insert(material.id(), MaterialGroup { group, _flags: flags, material: material.downgrade() });
        Ok(())
    }

    /// Release groups and images whose assets were dropped.
    pub fn release_dropped(&mut self) {
        self.groups.retain(|_, entry| entry.material.strong_count() > 0);
        self.textures.retain(|_, entry| entry.asset.strong_count() > 0);
    }

    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        asset: &TextureAsset,
        max_texture_dimension: u32,
    ) -> Result<(), RenderError> {
        if self.textures.contains_key(&asset.id()) {
            return Ok(());
        }
        if asset.width() > max_texture_dimension || asset.height() > max_texture_dimension {
            return Err(RenderError::InvalidTexture(format!(
                "PBR texture dimensions must be 1–{max_texture_dimension} pixels per axis."
            )));
        }
        let texture = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("material map"),
                size: wgpu::Extent3d { width: asset.width(), height: asset.height(), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            asset.pixels(),
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.textures.insert(asset.id(), UploadedTexture { view, _texture: texture, asset: asset.downgrade() });
        Ok(())
    }
}

/// A 1x1 texture of one color.
fn solid(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    color: [u8; 4],
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &color,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

/// A material bind group and its flags uniform.
fn group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    views: [&wgpu::TextureView; 6],
    has_normal_map: bool,
) -> (wgpu::BindGroup, wgpu::Buffer) {
    let flags = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("material flags"),
        contents: bytemuck::cast_slice(&[if has_normal_map { 1.0_f32 } else { 0.0 }, 0.0, 0.0, 0.0]),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let mut entries = vec![wgpu::BindGroupEntry { binding: 0, resource: flags.as_entire_binding() }];
    for (index, view) in views.iter().enumerate() {
        entries.push(wgpu::BindGroupEntry {
            binding: index as u32 + 1,
            resource: wgpu::BindingResource::TextureView(view),
        });
    }
    entries.push(wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(sampler) });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("material maps"),
        layout,
        entries: &entries,
    });
    (group, flags)
}
