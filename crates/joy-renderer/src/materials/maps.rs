// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! GPU copies of PBR material maps and their bind groups.
//!
//! Each [`MaterialTextures`] set gets one bind group (group 3 of the PBR pipelines, group 1 of the masked caster and
//! masked depth pre-pass) the first time a frame draws it; images upload once per color space and are shared between
//! sets. Missing maps bind white (or a flat normal), so untextured PBR surfaces use one shared default group.
//!
//! Color maps (base color and emission) upload as sRGB textures, so sampling decodes them and their mip chains average
//! linear light; data maps (normal, metallic, roughness, and occlusion) upload as UNORM and read linearly. Every image
//! gets a full mip chain and is sampled trilinearly with anisotropic filtering.

use std::collections::HashMap;
use std::sync::Weak;

use wgpu::util::DeviceExt;

use crate::RenderError;
use crate::gpu::GpuContext;
use crate::mesh::MaterialTextures;
use crate::texture::TextureAsset;
use crate::texture_mips::{
    MipmapGenerator,
    mipmapped_sampler,
};

/// Format of color maps, decoded from sRGB when sampled.
const COLOR_MAP_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Format of data maps, read as stored.
const DATA_MAP_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

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
    // Uploaded images keyed by asset identity and color space (true for sRGB color maps).
    textures: HashMap<(u64, bool), UploadedTexture>,
    groups: HashMap<u64, MaterialGroup>,
}

impl MaterialMaps {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout) -> Self {
        let sampler = mipmapped_sampler(device, "material maps", wgpu::AddressMode::Repeat);
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

    /// Upload a material's maps (with mip chains) and build its bind group the first time it is drawn.
    pub fn prepare(
        &mut self,
        gpu: &GpuContext,
        mipmaps: &mut MipmapGenerator,
        material: &MaterialTextures,
    ) -> Result<(), RenderError> {
        if self.groups.contains_key(&material.id()) {
            return Ok(());
        }
        let maps = material.maps();
        let color_maps = [&maps.base_color, &maps.emissive];
        let data_maps = [&maps.normal, &maps.metallic, &maps.roughness, &maps.ao];
        for texture in color_maps.iter().copied().flatten() {
            self.upload(gpu, mipmaps, texture, true)?;
        }
        for texture in data_maps.iter().copied().flatten() {
            self.upload(gpu, mipmaps, texture, false)?;
        }
        let view = |texture: &Option<TextureAsset>, srgb: bool, fallback: &'_ wgpu::TextureView| -> wgpu::TextureView {
            texture
                .as_ref()
                .and_then(|texture| self.textures.get(&(texture.id(), srgb)))
                .map_or(fallback, |uploaded| &uploaded.view)
                .clone()
        };
        let views = [
            view(&maps.base_color, true, &self.white.1),
            view(&maps.normal, false, &self.flat_normal.1),
            view(&maps.emissive, true, &self.white.1),
            view(&maps.metallic, false, &self.white.1),
            view(&maps.roughness, false, &self.white.1),
            view(&maps.ao, false, &self.white.1),
        ];
        let (group, flags) = group(gpu.device(), &self.layout, &self.sampler, views.each_ref(), maps.normal.is_some());
        self.groups.insert(material.id(), MaterialGroup { group, _flags: flags, material: material.downgrade() });
        Ok(())
    }

    /// Release groups and images whose assets were dropped.
    pub fn release_dropped(&mut self) {
        self.groups.retain(|_, entry| entry.material.strong_count() > 0);
        self.textures.retain(|_, entry| entry.asset.strong_count() > 0);
    }

    /// Upload one image in its color space with a mip chain, once.
    fn upload(
        &mut self,
        gpu: &GpuContext,
        mipmaps: &mut MipmapGenerator,
        asset: &TextureAsset,
        srgb: bool,
    ) -> Result<(), RenderError> {
        let key = (asset.id(), srgb);
        if self.textures.contains_key(&key) {
            return Ok(());
        }
        let max_texture_dimension = gpu.capabilities().max_texture_dimension;
        if asset.width() > max_texture_dimension || asset.height() > max_texture_dimension {
            return Err(RenderError::InvalidTexture(format!(
                "PBR texture dimensions must be 1–{max_texture_dimension} pixels per axis."
            )));
        }
        let format = if srgb { COLOR_MAP_FORMAT } else { DATA_MAP_FORMAT };
        let texture = mipmaps.upload(gpu, "material map", asset, format);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.textures.insert(key, UploadedTexture { view, _texture: texture, asset: asset.downgrade() });
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
